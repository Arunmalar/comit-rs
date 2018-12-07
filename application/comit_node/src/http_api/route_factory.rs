use futures::sync::mpsc::UnboundedSender;
use http_api::{self, rfc003::action::GetActionQueryParams};
use key_store::KeyStore;
use std::sync::Arc;
use swap_protocols::{
    rfc003::{self, state_store},
    MetadataStore,
};
use swaps::common::SwapId;
use warp::{self, filters::BoxedFilter, Filter, Reply};

#[derive(Copy, Clone, Debug)]
pub struct EnabledServices {
    pub lightning_bitcoin: bool,
}

pub fn create<T: MetadataStore<SwapId>, S: state_store::StateStore<SwapId>>(
    metadata_store: Arc<T>,
    state_store: Arc<S>,
    sender: UnboundedSender<(SwapId, rfc003::alice::SwapRequestKind)>,
    key_store: Arc<KeyStore>,
    enabled_services: EnabledServices,
) -> BoxedFilter<(impl Reply,)> {
    let path = warp::path(http_api::PATH);
    let rfc003 = path.and(warp::path(http_api::rfc003::swap::PROTOCOL_NAME));
    let metadata_store = warp::any().map(move || metadata_store.clone());
    let key_store = warp::any().map(move || key_store.clone());
    let state_store = warp::any().map(move || state_store.clone());
    let sender = warp::any().map(move || sender.clone());
    let enabled_services = warp::any().map(move || enabled_services);

    let rfc003_post_swap = rfc003
        .and(warp::path::end())
        .and(warp::post2())
        .and(key_store.clone())
        .and(sender)
        .and(enabled_services)
        .and(warp::body::json())
        .and_then(http_api::rfc003::swap::post_swap);

    let rfc003_get_swap = rfc003
        .and(warp::get2())
        .and(metadata_store.clone())
        .and(state_store.clone())
        .and(warp::path::param())
        .and(warp::path::end())
        .and_then(http_api::rfc003::swap::get_swap);

    let get_swaps = path
        .and(warp::get2())
        .and(warp::path::end())
        .and(metadata_store.clone())
        .and(state_store.clone())
        .and_then(http_api::rfc003::swap::get_swaps);

    let rfc003_post_action = rfc003
        .and(metadata_store.clone())
        .and(state_store.clone())
        .and(key_store.clone())
        .and(warp::path::param::<SwapId>())
        .and(warp::path::param::<http_api::rfc003::action::PostAction>())
        .and(warp::post2())
        .and(warp::path::end())
        .and(warp::body::json())
        .and_then(http_api::rfc003::action::post);

    let rfc003_get_action = rfc003
        .and(metadata_store.clone())
        .and(state_store.clone())
        .and(warp::path::param::<SwapId>())
        .and(warp::path::param::<http_api::rfc003::action::GetAction>())
        .and(warp::query::<GetActionQueryParams>())
        .and(warp::get2())
        .and(warp::path::end())
        .and_then(http_api::rfc003::action::get);

    rfc003_get_swap
        .or(rfc003_post_swap)
        .or(rfc003_post_action)
        .or(rfc003_get_action)
        .or(get_swaps)
        .with(warp::log("http"))
        .recover(http_api::unpack_problem)
        .boxed()
}
