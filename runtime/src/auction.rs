use support::{decl_storage, decl_module, decl_event, StorageMap, ensure, Parameter, traits::Currency};
use sr_primitives::traits::{SimpleArithmetic, Bounded, Member};
use system::ensure_signed;
use codec::{Encode, Decode};
use runtime_io::blake2_128;
use rstd::result;

pub trait Trait: system::Trait {
	type Event: From<Event<Self>> + Into<<Self as system::Trait>::Event>;
    type Currency: Currency<Self::AccountId>;
    type AuctionIndex: Parameter + Member + SimpleArithmetic + Bounded + Default + Copy;
}
type BalanceOf<T> = <<T as Trait>::Currency as Currency<<T as system::Trait>::AccountId>>::Balance;

#[derive(Encode, Decode, Clone, PartialEq)]
#[cfg_attr(feature = "std", derive(Debug))]
pub enum AuctionStatus {
    NotStarted, //未开卖
    Started, //正在拍卖（拍卖开始）
    Paused, //拍卖暂停
    Selled, // 拍卖成功
    Unselled, //流拍
}
impl Default for AuctionStatus {
    fn default() -> Self {
        AuctionStatus::NotStarted
    }
}

#[derive(Encode, Decode, Default, Clone, PartialEq)]
#[cfg_attr(feature = "std", derive(Debug))]
pub struct AuctionRecord<T> where T: Trait {
	record_id: [u8; 16], // 拍卖记录ID
    item_id: [u8; 16], // 拍卖品ID

    begin_time: u64, // 拍卖开始时间(时间戳)
    end_time: Option<u64>, // 拍卖结束时间(时间戳)

    start_price: BalanceOf<T>, // 起拍价
    current_price: BalanceOf<T>, // 当前价格
    bid_range: BalanceOf<T>, // 加价幅度

    status: AuctionStatus, // 拍卖品状态
    item_receiver: Option<<T as system::Trait>::AccountId>, //拍卖成功后，拍品的接收方
    item_seller: <T as system::Trait>::AccountId, //拍卖品收款方
}

decl_storage! {
	trait Store for Module<T: Trait> as Auctions {
        pub AuctionRecords get(record): map [u8;16] => Option<AuctionRecord<T>>; //存储 record_id => record
        pub RecordIds get(record_id): map (T::AccountId, [u8;16]) => [u8;16]; // 存储 (user, item_id) => (record_id)
        pub AuctionsItemRecord get(auction_item_record): map [u8; 16] => T::AccountId; // 存储 item_id => user
	}
}

decl_event!(
	pub enum Event<T>
    where <T as system::Trait>::AccountId, <T as system::Trait>::Hash {
        Created(AccountId, Hash),
    }
);

decl_module! {
	pub struct Module<T: Trait> for enum Call where origin: T::Origin {
		fn deposit_event() = default;

		/// 创建拍卖物品纪录（上市），各参数含义参照struct
		pub fn create_auction(origin, item_id: [u8; 16], begin_time: u64, start_price: BalanceOf<T>, bid_range: BalanceOf<T>, item_seller: T::AccountId) {
			let sender = ensure_signed(origin)?;

            // 参数检查
            ensure!((bid_range > <BalanceOf<T>>::from(0)), "加价幅度不可为0");
			
            // 1、判断当前物品是否在拍卖状态
            ensure!(!<AuctionsItemRecord<T>>::exists(&item_id), "此物品已在拍卖状态");


			// 2、当可被拍卖时，创建拍卖纪录
            let record_id = Self::random_value(&sender);
            let new_auction = AuctionRecord::<T> {
                record_id,
                item_id,
                begin_time,
                end_time: None,
                start_price,
                current_price: <BalanceOf<T>>::from(0),
                bid_range,
                status: AuctionStatus::NotStarted,
                item_receiver: None,
                item_seller: item_seller.clone(),
            };

            // 3、插入记录
            <AuctionRecords<T>>::insert(record_id, new_auction);
            <RecordIds<T>>::insert((item_seller.clone(), item_id), record_id);
            <AuctionsItemRecord<T>>::insert(item_id, item_seller);
		}

        /// 创建竞拍纪录
		pub fn create_auction_record(origin, auction_user: T::AccountId, record_id: [u8; 16]) {
			let sender = ensure_signed(origin)?;
			
			// 1、判断是否创建拍卖的人进行竞拍
			ensure!(sender == auction_user, "竞拍者不能为发布拍品人");
			// 2、判断拍品是否存在
			ensure!(<AuctionRecords<T>>::exists(&record_id), "不存在此拍卖");
			// 3、判断拍品状态
			let auction_record = Self::record(record_id).unwrap();
			ensure!(auction_record.status == AuctionStatus::NotStarted, "此拍卖品当前不可拍卖");

			let now: u64 = Self::get_current_time();
			ensure!(now >= auction_record.begin_time, "拍卖尚未开始");

			// 已超时不可拍卖
			if (!auction_record.end_time.is_some()) || (now > auction_record.end_time.unwrap()) {
                if auction_record.item_receiver.is_some() {
                    Self::change_auction_status(&sender, record_id, AuctionStatus::Selled)?;// 此处可不进行操作，正常应有定时操作进行时间方面的检查
                } else {
                    // 流拍，没有人购买
                    Self::change_auction_status(&sender, record_id, AuctionStatus::Unselled)?;
                }
			} else if auction_record.status == AuctionStatus::Started {
				// 未超时，且可进行拍卖
                //TODO 此时需要进行何种操作？auction_record中存在current_price，是否要修改？
                //auction_record

				// let current_price = Self::auction_price(record_id);
				// <AuctionsRecord<T>>::insert((record_id, auction_user), current_price + 1);
				// ActionPrice::insert(record_id, current_price + 1);
			}

		}

		/// 结算(应该为定时任务判断时间主动结束，此处采用用户手动结束方式)
		/// 若为定时任务模式，则需循环拍卖列表进行各自的判断
		pub fn auction_settle_accounts(origin, item_id: [u8; 16]) {
			let sender = ensure_signed(origin)?;

			// 1、判断拍卖
			ensure!(<AuctionsItemRecord<T>>::exists(&item_id), "不存在此拍卖");

			// 2、获取拍卖信息
			let item_seller = <AuctionsItemRecord<T>>::get(item_id);
            ensure!(<RecordIds<T>>::exists((item_seller.clone(), item_id)), "不存在此拍卖");

            let record_id = Self::record_id((item_seller.clone(), item_id));

            ensure!(Self::record(record_id).is_some(), "不存在此拍卖");
            let auction_record = Self::record(record_id).unwrap();

			let now = Self::get_current_time();

			if (auction_record.status == AuctionStatus::Selled) || (auction_record.status == AuctionStatus::Unselled) {
				//  若为已经完成拍卖，则结束不进行任何操作
			} else if auction_record.end_time.is_some() && (auction_record.end_time.unwrap() < now) {
				// 此条件中应放入定时任务
				if auction_record.current_price == auction_record.start_price {
					Self::change_auction_status(&sender, item_id, AuctionStatus::Unselled)?;
				} else {
					Self::change_auction_status(&sender, item_id, AuctionStatus::Selled)?; // 此处可不进行操作，正常应有定时操作进行时间方面的检查
				}
			} else {
				Self::change_auction_status(&sender, item_id, AuctionStatus::Selled)?;
			}
		}
	}
}

impl<T: Trait> Module<T> {
	/// 创建随机数用于标记唯一身份
	fn random_value(sender: &T::AccountId) -> [u8; 16] {
		let payload = (<system::Module<T>>::random_seed(), sender, <system::Module<T>>::extrinsic_index(), <system::Module<T>>::block_number());
		payload.using_encoded(blake2_128)
	}

    /// 修改拍卖物品状态(status = 3时拍卖终止)
	pub fn change_auction_status(sender: &T::AccountId, record_id: [u8; 16], status: AuctionStatus) -> result::Result<(), &'static str> {
		// let sender = ensure_signed(origin)?;

		// 1、判断是否拥有此拍卖纪录
        ensure!(Self::record(record_id).is_some(), "无此拍卖信息");

        let mut auction_record = Self::record(record_id).unwrap();
		ensure!(auction_record.item_seller != *sender, "用户无此拍卖信息");
		
		// 2、修改拍卖信息
		auction_record.status = status;
		<AuctionRecords<T>>::insert(record_id, auction_record);

		Ok(())
	}

    /// 获取当前时间
    pub fn get_current_time() -> u64 {
        // TODO: 获取当前时间
        0
    }
}