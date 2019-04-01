#![warn(unused_extern_crates, missing_debug_implementations, rust_2018_idioms)]
#![deny(unsafe_code)]

#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate strum_macros;

pub use bitcoin::{
	blockdata::{
		block::{Block, BlockHeader},
		opcodes,
		script::{self, Script},
		transaction::{OutPoint, SigHashType, Transaction, TxIn, TxOut},
	},
	network::serialize,
	util::{
		bip143::SighashComponents,
		bip32::{self, ChainCode, ChildNumber, ExtendedPrivKey, ExtendedPubKey, Fingerprint},
		hash::{self, Hash160, Sha256dHash, Sha256dHash as TransactionId, Sha256dHash as BlockId},
		privkey::Privkey as PrivateKey,
		Error,
	},
	Address,
};

pub use crate::{
	blocks::*,
	mined_block::*,
	network::*,
	pubkey::*,
	transaction::*,
	weight::{Error as WeightError, *},
};
pub use bitcoin_quantity::*;

mod blocks;
mod mined_block;
mod network;
mod pubkey;
mod transaction;
mod weight;
