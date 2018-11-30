#[macro_use]
mod transition_save;

pub mod actions;
pub mod alice;
pub mod bitcoin;
pub mod bob;
pub mod ethereum;
pub mod events;
pub mod find_htlc_location;
pub mod roles;
pub mod state_machine;
pub mod state_store;

mod error;

mod ledger;
mod outcome;
mod save_state;
mod secret;

#[cfg(test)]
mod state_machine_test;

pub use self::{
    error::Error,
    ledger::{
        FundTransaction, HttpRefundIdentity, HttpSuccessIdentity, IntoHtlcIdentity, Ledger,
        RedeemTransaction, RefundTransaction,
    },
    outcome::SwapOutcome,
    save_state::SaveState,
    secret::{RandomnessSource, Secret, SecretFromErr, SecretHash},
};
