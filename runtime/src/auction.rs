use support::{decl_storage, decl_module, StorageMap, ensure, decl_event};
use system::ensure_signed;
use codec::{Encode, Decode};
use runtime_io::blake2_128;
use rstd::result;

pub trait Trait: system::Trait {
	type Event: From<Event<Self>> + Into<<Self as system::Trait>::Event>;
}

// let step: u32 = 1; // 加价幅度

#[derive(Encode, Decode, Default, Clone, PartialEq)]
#[cfg_attr(feature = "std", derive(Debug))]
pub struct Auction {
	id: [u8; 16],
    kitty_dna: [u8; 16], // 拍卖品(kitty)
    begin_time: u16, // 拍卖开始时间(时间戳)
    end_time: u16, // 拍卖结束时间(时间戳)
    begin_price: u16, // 起拍价
    end_price: u16, // 拍卖结束时价格
    status: u8, // 拍卖品状态 0 拍卖成功，1 正在拍卖， 2 拍卖暂停， 3 流拍
	step: u16
}

decl_storage! {
	trait Store for Module<T: Trait> as Auctions {
		pub Auctions get(auction): map [u8; 16] => Option<Auction>; // 存储拍卖信息

		pub AuctionsOwner get(auctions_owner): map [u8; 16] => T::AccountId; // 存储拍卖信息与用户对应关系

		pub AuctionsItemRecord get(auction_item_record): map [u8; 16] => bool; // 存储拍卖品信息,key为拍卖品唯一识别code，value为任意真

		pub AuctionsRecord get(auction_record): map ([u8; 16], T::AccountId) => u16; // 存储拍卖品信息,key为拍卖品(auction_id)与拍卖用户组合的唯一code，value为step，此次功能中step为初始auction step值

		pub ActionPrice get(auction_price): map [u8; 16] => u16;
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
		pub fn create_auction(origin, kitty_dna: [u8; 16], begin_price: u16, end_price: u16, begin_time: u16, end_time: u16, step: u16) {
			let sender = ensure_signed(origin)?;

			let auction_id = Self::random_value(&sender);

			// 1、判断当前物品是否在拍卖状态
			ensure!(AuctionsItemRecord::exists(&kitty_dna), "此kitty已在拍卖状态");

			// 2、当可被拍卖时，创建拍卖纪录
			// 参数时间格式为时间戳，此处应该对时间戳进行判断
			// 若开始时间为空则默认 begin_time = now，若结束时间为空则禁止创建
			// 若未设置终止拍卖价格，则默认为0，即为无上限

			let mut final_step = step;

			if final_step < 1 {
				final_step = 1;
			}

			let new_auction = Auction {
				id: auction_id,
				kitty_dna: kitty_dna,
				begin_time: begin_time,
				end_time: end_time,
				begin_price: begin_price,
				end_price: end_price,
				status: 1,
				step: final_step
			};

			// 3、将拍卖信息记录，并记录当前物品到不可被拍卖列表
			Auctions::insert(auction_id, new_auction);
			ActionPrice::insert(auction_id, begin_price);
			<AuctionsOwner<T>>::insert(auction_id, sender);
			AuctionsItemRecord::insert(kitty_dna, true);

		}

		/// 创建竞拍纪录
		pub fn create_auction_record(origin, auction_user: T::AccountId, auction_id: [u8; 16]) {
			let sender = ensure_signed(origin)?;
			
			// 1、判断是否创建拍卖的人进行竞拍
			ensure!(sender == auction_user, "竞拍者不能为发布拍品人");
			// 2、判断拍品是否存在
			ensure!(!Auctions::exists(&auction_id), "不存在此拍卖");
			// 3、判断拍品状态
			let auction_info = Self::auction(auction_id).unwrap();
			// 拍卖品状态 0 拍卖成功，1 正在拍卖， 2 拍卖暂停， 3 流拍
			ensure!(auction_info.status != 1, "此拍卖品当前不可拍卖");

			let now: u16 = 0; // TODO 此处now应为当前时间，暂未查询substrate时间使用方式
			ensure!(auction_info.begin_time - now > 0, "拍卖尚未开始");

			// 已超时不可拍卖
			if auction_info.end_time - now < 0{
				// 流拍，没有人进行拍卖
				if auction_info.end_price - auction_info.begin_price == 0 {
					Self::change_auction_status(&sender, auction_id, 3);
				} else {
					Self::change_auction_status(&sender, auction_id, 0); // 此处可不进行操作，正常应有定时操作进行时间方面的检查
				}
			} else if auction_info.status == 1 {
				// 未超时，且可进行拍卖
				let current_price = Self::auction_price(auction_id);
				<AuctionsRecord<T>>::insert((auction_id, auction_user), current_price + 1);
				ActionPrice::insert(auction_id, current_price + 1);
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
	pub fn change_auction_status(sender: &T::AccountId, auction_id: [u8; 16], status: u8) -> result::Result<&'static str, &'static str> {
		// let sender = ensure_signed(origin)?;

		// 1、判断是否拥有此拍卖纪录
		ensure!(<AuctionsOwner<T>>::exists(auction_id), "无此拍卖信息");
		let owner_info = Self::auctions_owner(auction_id);
		ensure!(owner_info != *sender, "用户无此拍卖信息");
		// 2、获取拍卖信息
		let auction_info = Self::auction(auction_id);
		ensure!(auction_info.is_some(), "信息错误");
		let mut auction_info = auction_info.unwrap();
		
		// 3、修改拍卖信息
		auction_info.status = status;
		Auctions::insert(auction_id, auction_info);

		Ok("ok")
	}
}