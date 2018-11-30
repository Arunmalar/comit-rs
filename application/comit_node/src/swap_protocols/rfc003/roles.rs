use comit_client::SwapReject;
use futures::{sync::oneshot, Future};
use std::{
    fmt::Debug,
    marker::PhantomData,
    sync::{Arc, Mutex},
};
use swap_protocols::rfc003::{events::ResponseFuture, state_machine::StateMachineResponse};

use swap_protocols::{
    self,
    asset::Asset,
    rfc003::{
        actions::bob::{Accept, Decline},
        ledger::Ledger,
        Secret, SecretHash,
    },
};

pub trait Role: Send + Sync + Debug + Clone + 'static {
    type AlphaLedger: Ledger;
    type BetaLedger: Ledger;
    type AlphaAsset: Asset;
    type BetaAsset: Asset;
    type AlphaSuccessHtlcIdentity: Send
        + Sync
        + Clone
        + Debug
        + PartialEq
        + Into<<Self::AlphaLedger as swap_protocols::Ledger>::Identity>;

    type AlphaRefundHtlcIdentity: Send
        + Sync
        + Clone
        + Debug
        + PartialEq
        + Into<<Self::AlphaLedger as swap_protocols::Ledger>::Identity>;

    type BetaSuccessHtlcIdentity: Send
        + Sync
        + Clone
        + Debug
        + PartialEq
        + Into<<Self::BetaLedger as swap_protocols::Ledger>::Identity>;

    type BetaRefundHtlcIdentity: Send
        + Sync
        + Clone
        + Debug
        + PartialEq
        + Into<<Self::BetaLedger as swap_protocols::Ledger>::Identity>;

    type Secret: Send + Sync + Clone + Into<SecretHash> + Debug + PartialEq;
}

#[derive(Clone, Debug)]
pub struct Alice<AL, BL, AA, BA> {
    phantom_data: PhantomData<(AL, BL, AA, BA)>,
}

impl<AL, BL, AA, BA> Alice<AL, BL, AA, BA> {
    pub fn new() -> Self {
        Self {
            phantom_data: PhantomData,
        }
    }
}

impl<AL: Ledger, BL: Ledger, AA: Asset, BA: Asset> Role for Alice<AL, BL, AA, BA> {
    type AlphaLedger = AL;
    type BetaLedger = BL;
    type AlphaAsset = AA;
    type BetaAsset = BA;
    type AlphaSuccessHtlcIdentity = AL::Identity;
    type AlphaRefundHtlcIdentity = AL::HtlcIdentity;
    type BetaSuccessHtlcIdentity = BL::HtlcIdentity;
    type BetaRefundHtlcIdentity = BL::Identity;
    type Secret = Secret;
}

#[derive(Debug, Clone)]
pub struct Bob<AL: Ledger, BL: Ledger, AA, BA> {
    phantom_data: PhantomData<(AL, BL, AA, BA)>,
    response_sender: Arc<
        Mutex<
            Option<
                oneshot::Sender<
                    Result<
                        StateMachineResponse<AL::HtlcIdentity, BL::HtlcIdentity, BL::LockDuration>,
                        SwapReject,
                    >,
                >,
            >,
        >,
    >,
}

impl<AL: Ledger, BL: Ledger, AA: Asset, BA: Asset> Bob<AL, BL, AA, BA> {
    pub fn new() -> (Self, Box<ResponseFuture<Self>>) {
        let (sender, receiver) = oneshot::channel();
        (
            Bob {
                phantom_data: PhantomData,
                response_sender: Arc::new(Mutex::new(Some(sender))),
            },
            Box::new(
                receiver
                    .map_err(|_e| unreachable!("For now, it should be impossible for the sender to go out of scope before the receiver") ),
            ),
        )
    }

    pub fn accept_action(&self) -> Accept<AL, BL> {
        Accept::new(self.response_sender.clone())
    }

    pub fn decline_action(&self) -> Decline<AL, BL> {
        Decline::new(self.response_sender.clone())
    }
}

impl<AL: Ledger, BL: Ledger, AA: Asset, BA: Asset> Role for Bob<AL, BL, AA, BA> {
    type AlphaLedger = AL;
    type BetaLedger = BL;
    type AlphaAsset = AA;
    type BetaAsset = BA;
    type AlphaSuccessHtlcIdentity = AL::HtlcIdentity;
    type AlphaRefundHtlcIdentity = AL::Identity;
    type BetaSuccessHtlcIdentity = BL::Identity;
    type BetaRefundHtlcIdentity = BL::HtlcIdentity;
    type Secret = SecretHash;
}

#[cfg(test)]
pub mod test {
    use super::*;
    use bitcoin_support::BitcoinQuantity;
    use comit_client;
    use ethereum_support::EtherQuantity;
    use swap_protocols::{
        ledger::{Bitcoin, Ethereum},
        rfc003::events::{CommunicationEvents, ResponseFuture},
    };

    pub type Alisha = Alice<Bitcoin, Ethereum, BitcoinQuantity, EtherQuantity>;
    pub type Bobisha = Bob<Bitcoin, Ethereum, BitcoinQuantity, EtherQuantity>;

    impl PartialEq<Alisha> for Alisha {
        fn eq(&self, _: &Alisha) -> bool {
            true
        }
    }

    impl PartialEq<Bobisha> for Bobisha {
        fn eq(&self, _: &Bobisha) -> bool {
            true
        }
    }

    #[allow(missing_debug_implementations)]
    pub struct FakeCommunicationEvents<R: Role> {
        pub response: Option<Box<ResponseFuture<R>>>,
    }

    impl<R: Role> CommunicationEvents<R> for FakeCommunicationEvents<R> {
        fn request_responded(
            &mut self,
            _request: &comit_client::rfc003::Request<
                R::AlphaLedger,
                R::BetaLedger,
                R::AlphaAsset,
                R::BetaAsset,
            >,
        ) -> &mut ResponseFuture<R> {
            self.response.as_mut().unwrap()
        }
    }

}
