#![warn(rust_2018_idioms)]
#![deny(unsafe_code)]

pub mod bitcoin;
pub mod ethereum;
mod in_memory_query_repository;
mod in_memory_query_result_repository;
pub mod load_settings;
pub mod logging;
mod query_repository;
mod query_result_repository;
pub mod route_factory;
mod routes;
pub mod settings;

pub use crate::{
    in_memory_query_repository::*, in_memory_query_result_repository::*, query_repository::*,
    query_result_repository::*, route_factory::*, routes::*,
};
pub use ethereum_support::web3;
use std::{cmp::Ordering, sync::Arc};

#[derive(PartialEq, PartialOrd)]
pub struct QueryId(pub u32);
#[derive(PartialEq)]
pub struct QueryMatch(pub QueryId, pub String);

type ArcQueryRepository<Q> = Arc<dyn QueryRepository<Q>>;

impl From<u32> for QueryId {
    fn from(item: u32) -> Self {
        Self(item)
    }
}

impl PartialOrd for QueryMatch {
    fn partial_cmp(&self, other: &QueryMatch) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}
