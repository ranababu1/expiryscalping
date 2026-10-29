//! The Kite Connect broker. It arrives in Phase 3 (live trading with tiny
//! size). Until then every call returns `NotAvailable`, and the engine
//! refuses live mode, so no code path can reach Zerodha's order API.

use crate::*;

#[derive(Debug, Default)]
pub struct KiteBroker;

const PHASE3: &str = "the Kite broker arrives in Phase 3";

#[async_trait]
impl Broker for KiteBroker {
    async fn place(&self, _req: &OrderRequest) -> Result<BrokerOrderId, BrokerError> {
        Err(BrokerError::NotAvailable(PHASE3))
    }
    async fn modify(&self, _id: &BrokerOrderId, _m: ApprovedModify) -> Result<(), BrokerError> {
        Err(BrokerError::NotAvailable(PHASE3))
    }
    async fn cancel(&self, _id: &BrokerOrderId) -> Result<(), BrokerError> {
        Err(BrokerError::NotAvailable(PHASE3))
    }
    async fn orders(&self) -> Result<Vec<BrokerOrder>, BrokerError> {
        Err(BrokerError::NotAvailable(PHASE3))
    }
    async fn positions(&self) -> Result<Vec<Position>, BrokerError> {
        Err(BrokerError::NotAvailable(PHASE3))
    }
    async fn margins(&self) -> Result<Margins, BrokerError> {
        Err(BrokerError::NotAvailable(PHASE3))
    }
}
