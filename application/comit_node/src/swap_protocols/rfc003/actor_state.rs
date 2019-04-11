use crate::{
    comit_client::SwapReject,
    swap_protocols::{
        asset::Asset,
        rfc003::{self, ledger_state::LedgerState, messages::*, Ledger},
    },
};
use blockchain_contracts::rfc003::secret::Secret;
use std::fmt::Debug;

pub trait ActorState: Debug + Clone + Send + Sync + 'static {
    type AL: Ledger;
    type BL: Ledger;
    type AA: Asset;
    type BA: Asset;

    fn set_response(
        &mut self,
        response: Result<AcceptResponseBody<Self::AL, Self::BL>, SwapReject>,
    );
    fn set_secret(&mut self, secret: Secret);
    fn set_error(&mut self, error: rfc003::Error);
    fn alpha_ledger_mut(&mut self) -> &mut LedgerState<Self::AL>;
    fn beta_ledger_mut(&mut self) -> &mut LedgerState<Self::BL>;
}
