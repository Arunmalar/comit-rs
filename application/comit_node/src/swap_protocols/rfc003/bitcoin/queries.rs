use bitcoin_support::BitcoinQuantity;
use bitcoin_support::OutPoint;
use ledger_query_service::BitcoinQuery;
use swap_protocols::{
    asset::Asset,
    ledger::Bitcoin,
    rfc003::{
        bitcoin::bitcoin_htlc_address, events::NewSourceHtlcFundedQuery,
        events::NewSourceHtlcRefundedQuery,
        state_machine::OngoingSwap, Ledger, SecretHash,
    },
};

impl<TL, TA, S> NewSourceHtlcFundedQuery<Bitcoin, TL, BitcoinQuantity, TA, S> for BitcoinQuery
where
    TL: Ledger,
    TA: Asset,
    S: Into<SecretHash> + Clone,
{
    fn new_source_htlc_funded_query(
        swap: &OngoingSwap<Bitcoin, TL, BitcoinQuantity, TA, S>,
    ) -> Self {
        BitcoinQuery::Transaction {
            to_address: Some(bitcoin_htlc_address(swap)),
            unlock_script: None
        }
    }
}
