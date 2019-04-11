use crate::swap_protocols::{
    ledger::{Bitcoin, Ethereum},
    rfc003::{
        self,
        alice::{self, SwapCommunication},
        bitcoin, ethereum,
        secret_source::SecretSource,
        state_machine::HtlcParams,
        Action, Actions, LedgerState,
    },
};
use bitcoin_support::{BitcoinQuantity, OutPoint};
use bitcoin_witness::PrimedInput;
use blockchain_contracts::{ethereum::rfc003::Erc20Htlc, rfc003::secret::Secret};
use ethereum_support::{Bytes, Erc20Token, EtherQuantity};

type Request = rfc003::messages::Request<Bitcoin, Ethereum, BitcoinQuantity, Erc20Token>;
type Response = rfc003::messages::AcceptResponseBody<Bitcoin, Ethereum>;

fn fund_action(request: &Request, response: &Response) -> bitcoin::SendToAddress {
    let to = HtlcParams::new_alpha_params(request, response).compute_address();
    let amount = request.alpha_asset;
    let network = request.alpha_ledger.network;

    bitcoin::SendToAddress {
        to,
        amount,
        network,
    }
}

fn refund_action(
    request: &Request,
    response: &Response,
    alpha_htlc_location: OutPoint,
    secret_source: &dyn SecretSource,
) -> bitcoin::SpendOutput {
    let alpha_asset = request.alpha_asset;
    let htlc = bitcoin::Htlc::from(HtlcParams::new_alpha_params(request, response));
    let network = request.alpha_ledger.network;

    bitcoin::SpendOutput {
        output: PrimedInput::new(
            alpha_htlc_location,
            alpha_asset,
            htlc.unlock_after_timeout(secret_source.secp256k1_refund()),
        ),
        network,
    }
}

fn redeem_action(
    request: &Request,
    beta_htlc_location: ethereum_support::Address,
    secret: Secret,
) -> ethereum::SendTransaction {
    let data = Bytes::from(secret.raw_secret().to_vec());
    let gas_limit = Erc20Htlc::tx_gas_limit();
    let network = request.beta_ledger.network;

    ethereum::SendTransaction {
        to: beta_htlc_location,
        data,
        gas_limit,
        amount: EtherQuantity::zero(),
        network,
    }
}

impl Actions for alice::State<Bitcoin, Ethereum, BitcoinQuantity, Erc20Token> {
    type ActionKind = alice::ActionKind<
        (),
        bitcoin::SendToAddress,
        ethereum::SendTransaction,
        bitcoin::SpendOutput,
    >;

    fn actions(&self) -> Vec<Action<Self::ActionKind>> {
        let (request, response) = match self.swap_communication {
            SwapCommunication::Accepted {
                ref request,
                ref response,
            } => (request, response),
            _ => return vec![],
        };
        let alpha_state = &self.alpha_ledger_state;
        let beta_state = &self.beta_ledger_state;

        use self::LedgerState::*;

        let mut actions = match alpha_state {
            NotDeployed => {
                vec![alice::ActionKind::Fund(fund_action(&request, &response)).into_action()]
            }
            Funded { htlc_location, .. } => vec![alice::ActionKind::Refund(refund_action(
                &request,
                &response,
                *htlc_location,
                &*self.secret_source,
            ))
            .into_action()
            .with_invalid_until(request.alpha_expiry)],
            _ => vec![],
        };

        if let Funded { htlc_location, .. } = beta_state {
            actions.push(
                alice::ActionKind::Redeem(redeem_action(
                    &request,
                    *htlc_location,
                    self.secret_source.secret(),
                ))
                .into_action(),
            );
        }
        actions
    }
}
