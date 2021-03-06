use crate::{success_response, ApiError, ApiResult, NetworkService};
use beacon_chain::BeaconChainTypes;
use eth2_libp2p::{Enr, Multiaddr, PeerId};
use hyper::{Body, Request};
use std::sync::Arc;

/// HTTP handle to return the list of libp2p multiaddr the client is listening on.
///
/// Returns a list of `Multiaddr`, serialized according to their `serde` impl.
pub fn get_listen_addresses<T: BeaconChainTypes>(req: Request<Body>) -> ApiResult {
    let network = req
        .extensions()
        .get::<Arc<NetworkService<T>>>()
        .ok_or_else(|| ApiError::ServerError("NetworkService extension missing".to_string()))?;

    let multiaddresses: Vec<Multiaddr> = network.listen_multiaddrs();

    Ok(success_response(Body::from(
        serde_json::to_string(&multiaddresses)
            .map_err(|e| ApiError::ServerError(format!("Unable to serialize Enr: {:?}", e)))?,
    )))
}

/// HTTP handle to return the list of libp2p multiaddr the client is listening on.
///
/// Returns a list of `Multiaddr`, serialized according to their `serde` impl.
pub fn get_listen_port<T: BeaconChainTypes>(req: Request<Body>) -> ApiResult {
    let network = req
        .extensions()
        .get::<Arc<NetworkService<T>>>()
        .ok_or_else(|| ApiError::ServerError("NetworkService extension missing".to_string()))?;

    Ok(success_response(Body::from(
        serde_json::to_string(&network.listen_port())
            .map_err(|e| ApiError::ServerError(format!("Unable to serialize port: {:?}", e)))?,
    )))
}

/// HTTP handle to return the Discv5 ENR from the client's libp2p service.
///
/// ENR is encoded as base64 string.
pub fn get_enr<T: BeaconChainTypes>(req: Request<Body>) -> ApiResult {
    let network = req
        .extensions()
        .get::<Arc<NetworkService<T>>>()
        .ok_or_else(|| ApiError::ServerError("NetworkService extension missing".to_string()))?;

    let enr: Enr = network.local_enr();

    Ok(success_response(Body::from(
        serde_json::to_string(&enr.to_base64())
            .map_err(|e| ApiError::ServerError(format!("Unable to serialize Enr: {:?}", e)))?,
    )))
}

/// HTTP handle to return the `PeerId` from the client's libp2p service.
///
/// PeerId is encoded as base58 string.
pub fn get_peer_id<T: BeaconChainTypes>(req: Request<Body>) -> ApiResult {
    let network = req
        .extensions()
        .get::<Arc<NetworkService<T>>>()
        .ok_or_else(|| ApiError::ServerError("NetworkService extension missing".to_string()))?;

    let peer_id: PeerId = network.local_peer_id();

    Ok(success_response(Body::from(
        serde_json::to_string(&peer_id.to_base58())
            .map_err(|e| ApiError::ServerError(format!("Unable to serialize Enr: {:?}", e)))?,
    )))
}

/// HTTP handle to return the number of peers connected in the client's libp2p service.
pub fn get_peer_count<T: BeaconChainTypes>(req: Request<Body>) -> ApiResult {
    let network = req
        .extensions()
        .get::<Arc<NetworkService<T>>>()
        .ok_or_else(|| ApiError::ServerError("NetworkService extension missing".to_string()))?;

    let connected_peers: usize = network.connected_peers();

    Ok(success_response(Body::from(
        serde_json::to_string(&connected_peers)
            .map_err(|e| ApiError::ServerError(format!("Unable to serialize Enr: {:?}", e)))?,
    )))
}

/// HTTP handle to return the list of peers connected to the client's libp2p service.
///
/// Peers are presented as a list of `PeerId::to_string()`.
pub fn get_peer_list<T: BeaconChainTypes>(req: Request<Body>) -> ApiResult {
    let network = req
        .extensions()
        .get::<Arc<NetworkService<T>>>()
        .ok_or_else(|| ApiError::ServerError("NetworkService extension missing".to_string()))?;

    let connected_peers: Vec<String> = network
        .connected_peer_set()
        .iter()
        .map(PeerId::to_string)
        .collect();

    Ok(success_response(Body::from(
        serde_json::to_string(&connected_peers).map_err(|e| {
            ApiError::ServerError(format!("Unable to serialize Vec<PeerId>: {:?}", e))
        })?,
    )))
}
