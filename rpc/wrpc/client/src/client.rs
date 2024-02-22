use crate::imports::*;
use crate::parse::parse_host;
use crate::{error::Error, node::NodeDescriptor};
use kaspa_consensus_core::network::NetworkType;
use kaspa_rpc_core::{
    api::ctl::RpcCtl,
    notify::collector::{RpcCoreCollector, RpcCoreConverter},
};
pub use kaspa_rpc_macros::build_wrpc_client_interface;
use std::fmt::Debug;
use workflow_core::{channel::Multiplexer, runtime as application_runtime};
use workflow_dom::utils::window;
use workflow_rpc::client::Ctl as WrpcCtl;
pub use workflow_rpc::client::{
    ConnectOptions, ConnectResult, ConnectStrategy, Resolver as RpcResolver, ResolverResult, WebSocketConfig, WebSocketError,
};

// /// [`NotificationMode`] controls notification delivery process
// #[wasm_bindgen]
// #[derive(Clone, Copy, Debug)]
// pub enum NotificationMode {
//     /// Local notifier is used for notification processing.
//     ///
//     /// Multiple listeners can register and subscribe independently.
//     MultiListeners,
//     /// No notifier is present, notifications are relayed
//     /// directly through the internal channel to a single listener.
//     Direct,
// }

struct Inner {
    rpc_client: Arc<RpcClient<RpcApiOps>>,
    notification_channel: Channel<Notification>,
    encoding: Encoding,
    wrpc_ctl_multiplexer: Multiplexer<WrpcCtl>,
    rpc_ctl: RpcCtl,
    background_services_running: Arc<AtomicBool>,
    service_ctl: DuplexChannel<()>,
    // ---
    default_url: Mutex<Option<String>>,
    current_url: Mutex<Option<String>>,
    resolver: Option<Resolver>,
    network_id: Option<NetworkId>,
    node_descriptor: Mutex<Option<Arc<NodeDescriptor>>>,
}

impl Inner {
    pub fn new(encoding: Encoding, url: Option<&str>, resolver: Option<Resolver>, network_id: Option<NetworkId>) -> Result<Inner> {
        // log_trace!("Kaspa wRPC::{encoding} connecting to: {url}");
        let rpc_ctl = RpcCtl::with_descriptor(url);
        let wrpc_ctl_multiplexer = Multiplexer::<WrpcCtl>::new();

        let options = RpcClientOptions::new().with_ctl_multiplexer(wrpc_ctl_multiplexer.clone());

        let notification_channel = Channel::unbounded();

        // The `Interface` struct can be used to register for server-side
        // notifications. All notification methods have to be created at
        // this stage.
        let mut interface = Interface::<RpcApiOps>::new();

        [
            RpcApiOps::BlockAddedNotification,
            RpcApiOps::VirtualChainChangedNotification,
            RpcApiOps::FinalityConflictNotification,
            RpcApiOps::FinalityConflictResolvedNotification,
            RpcApiOps::UtxosChangedNotification,
            RpcApiOps::SinkBlueScoreChangedNotification,
            RpcApiOps::VirtualDaaScoreChangedNotification,
            RpcApiOps::PruningPointUtxoSetOverrideNotification,
            RpcApiOps::NewBlockTemplateNotification,
        ]
        .into_iter()
        .for_each(|notification_op| {
            let notification_sender_ = notification_channel.sender.clone();
            interface.notification(
                notification_op,
                workflow_rpc::client::Notification::new(move |notification: kaspa_rpc_core::Notification| {
                    let notification_sender = notification_sender_.clone();
                    Box::pin(async move {
                        // log_info!("notification receivers: {}", notification_sender.receiver_count());
                        // log_trace!("notification {:?}", notification);
                        if notification_sender.receiver_count() > 1 {
                            // log_info!("notification: posting to channel: {notification:?}");
                            notification_sender.send(notification).await?;
                        } else {
                            log_warning!("WARNING: Kaspa RPC notification is not consumed by user: {:?}", notification);
                        }
                        Ok(())
                    })
                }),
            );
        });

        let rpc = Arc::new(RpcClient::new_with_encoding(encoding, interface.into(), options, None)?);
        let client = Self {
            rpc_client: rpc,
            notification_channel,
            encoding,
            wrpc_ctl_multiplexer,
            rpc_ctl,
            service_ctl: DuplexChannel::unbounded(),
            background_services_running: Arc::new(AtomicBool::new(false)),
            // ---
            default_url: Mutex::new(url.map(|s| s.to_string())),
            current_url: Mutex::new(None),
            resolver,
            network_id,
            node_descriptor: Mutex::new(None),
        };
        Ok(client)
    }

    pub fn notification_channel_receiver(&self) -> Receiver<Notification> {
        self.notification_channel.receiver.clone()
    }

    pub fn shutdown_notification_channel(&self) -> bool {
        self.notification_channel.receiver.close()
    }

    /// Start sending notifications of some type to the client.
    async fn start_notify_to_client(&self, scope: Scope) -> RpcResult<()> {
        let _response: SubscribeResponse = self.rpc_client.call(RpcApiOps::Subscribe, scope).await.map_err(|err| err.to_string())?;
        Ok(())
    }

    /// Stop sending notifications of some type to the client.
    async fn stop_notify_to_client(&self, scope: Scope) -> RpcResult<()> {
        let _response: UnsubscribeResponse =
            self.rpc_client.call(RpcApiOps::Unsubscribe, scope).await.map_err(|err| err.to_string())?;
        Ok(())
    }

    fn default_url(&self) -> Option<String> {
        self.default_url.lock().unwrap().clone()
    }

    fn set_default_url(&self, url: Option<&str>) {
        *self.default_url.lock().unwrap() = url.map(String::from);
    }

    fn current_url(&self) -> Option<String> {
        self.current_url.lock().unwrap().clone()
    }

    fn set_current_url(&self, url: Option<&str>) {
        *self.current_url.lock().unwrap() = url.map(String::from);
    }
}

impl Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KaspaRpcClient")
            .field("rpc", &"rpc")
            .field("notification_channel", &self.notification_channel)
            .field("encoding", &self.encoding)
            .finish()
    }
}

#[async_trait]
impl SubscriptionManager for Inner {
    async fn start_notify(&self, _: ListenerId, scope: Scope) -> NotifyResult<()> {
        // log_trace!("[WrpcClient] start_notify: {:?}", scope);
        self.start_notify_to_client(scope).await.map_err(|err| NotifyError::General(err.to_string()))?;
        Ok(())
    }

    async fn stop_notify(&self, _: ListenerId, scope: Scope) -> NotifyResult<()> {
        // log_trace!("[WrpcClient] stop_notify: {:?}", scope);
        self.stop_notify_to_client(scope).await.map_err(|err| NotifyError::General(err.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl RpcResolver for Inner {
    async fn resolve_url(&self) -> ResolverResult {
        let url = if let Some(url) = self.default_url() {
            url
        } else if let Some(resolver) = self.resolver.as_ref() {
            let network_id = self.network_id.expect("Beacon requires network id in RPC client configuration");
            let node = resolver.get_node(self.encoding, network_id).await.map_err(WebSocketError::custom)?;
            let url = node.url.clone();
            self.node_descriptor.lock().unwrap().replace(Arc::new(node));
            url
        } else {
            panic!("RpcClient resolver configuration error (expecting Some(Beacon))")
        };

        self.rpc_ctl.set_descriptor(Some(url.clone()));
        self.set_current_url(Some(&url));
        Ok(url)
    }
}

const WRPC_CLIENT: &str = "wrpc-client";

/// [`KaspaRpcClient`] allows connection to the Kaspa wRPC Server via
/// binary Borsh or JSON protocols.
///
/// RpcClient has two ways to interface with the underlying RPC subsystem:
/// [`Interface`] that has a [`notification()`](Interface::notification)
/// method to register closures that will be invoked on server-side
/// notifications and the [`RpcClient::call`] method that allows async
/// method invocation server-side.
///
#[derive(Clone)]
pub struct KaspaRpcClient {
    inner: Arc<Inner>,
    notifier: Option<Arc<Notifier<Notification, ChannelConnection>>>,
    notification_mode: NotificationMode,
}

impl Debug for KaspaRpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KaspaRpcClient").field("url", &self.url()).field("connected", &self.is_connected()).finish()
    }
}

impl KaspaRpcClient {
    /// Create a new `KaspaRpcClient` with the given Encoding and URL
    pub fn new(
        encoding: Encoding,
        url: Option<&str>,
        resolver: Option<Resolver>,
        network_id: Option<NetworkId>,
    ) -> Result<KaspaRpcClient> {
        Self::new_with_args(encoding, NotificationMode::Direct, url, resolver, network_id)
    }

    /// Extended constructor that accepts [`NotificationMode`] argument.
    pub fn new_with_args(
        encoding: Encoding,
        notification_mode: NotificationMode,
        url: Option<&str>,
        resolver: Option<Resolver>,
        network_id: Option<NetworkId>,
    ) -> Result<KaspaRpcClient> {
        let inner = Arc::new(Inner::new(encoding, url, resolver, network_id)?);
        let notifier = if matches!(notification_mode, NotificationMode::MultiListeners) {
            let enabled_events = EVENT_TYPE_ARRAY[..].into();
            let converter = Arc::new(RpcCoreConverter::new());
            let collector = Arc::new(RpcCoreCollector::new(WRPC_CLIENT, inner.notification_channel_receiver(), converter));
            let subscriber = Arc::new(Subscriber::new(WRPC_CLIENT, enabled_events, inner.clone(), 0));
            Some(Arc::new(Notifier::new(WRPC_CLIENT, enabled_events, vec![collector], vec![subscriber], 3)))
        } else {
            None
        };

        let client = KaspaRpcClient { inner, notifier, notification_mode };

        Ok(client)
    }

    pub fn url(&self) -> Option<String> {
        self.inner.current_url()
    }

    pub fn set_url(&self, url: Option<&str>) -> Result<()> {
        self.inner.set_default_url(url);
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.inner.rpc_client.is_open()
    }

    pub fn encoding(&self) -> Encoding {
        self.inner.encoding
    }

    pub fn resolver(&self) -> Option<Resolver> {
        self.inner.resolver.clone()
    }

    pub fn node_descriptor(&self) -> Option<Arc<NodeDescriptor>> {
        self.inner.node_descriptor.lock().unwrap().clone()
    }

    pub fn rpc_client(&self) -> &Arc<RpcClient<RpcApiOps>> {
        &self.inner.rpc_client
    }

    pub fn rpc_api(self: &Arc<Self>) -> Arc<dyn RpcApi> {
        self.clone()
    }

    pub fn rpc_ctl(&self) -> &RpcCtl {
        &self.inner.rpc_ctl
    }

    /// Starts RPC services.
    pub async fn start(&self) -> Result<()> {
        if !self.inner.background_services_running.load(Ordering::SeqCst) {
            match &self.notification_mode {
                NotificationMode::MultiListeners => {
                    self.notifier.clone().unwrap().start();
                }
                NotificationMode::Direct => {}
            }

            self.start_rpc_ctl_service().await?;
        }
        Ok(())
    }

    /// Stops background services.
    pub async fn stop(&self) -> Result<()> {
        if self.inner.background_services_running.load(Ordering::SeqCst) {
            match &self.notification_mode {
                NotificationMode::MultiListeners => {
                    self.inner.shutdown_notification_channel();
                    self.notifier.as_ref().unwrap().join().await?;
                }
                NotificationMode::Direct => {
                    // self.notification_ctl.signal(()).await?;
                }
            }

            self.stop_rpc_ctl_service().await?;
        }
        Ok(())
    }

    /// Starts a background async connection task connecting
    /// to the wRPC server.  If the supplied `block` call is `true`
    /// this function will block until the first successful
    /// connection.
    pub async fn connect(&self, options: Option<ConnectOptions>) -> ConnectResult<Error> {
        let mut options = options.unwrap_or_default();

        if let Some(url) = options.url.take() {
            self.set_url(Some(&url))?;
        }

        // 1Gb message and frame size limits (on native and NodeJs platforms)
        let ws_config = WebSocketConfig {
            max_message_size: Some(1024 * 1024 * 1024),
            max_frame_size: Some(1024 * 1024 * 1024),
            accept_unmasked_frames: false,
            resolver: Some(self.inner.clone()),
            ..Default::default()
        };

        self.start().await?;
        self.inner.rpc_client.configure(ws_config);
        Ok(self.inner.rpc_client.connect(options).await?)
    }

    pub async fn disconnect(&self) -> Result<()> {
        self.inner.rpc_client.shutdown().await?;
        self.stop().await?;
        Ok(())
    }

    /// Stop and shutdown RPC disconnecting existing connections
    /// and stopping reconnection process.
    pub async fn shutdown(&self) -> Result<()> {
        Ok(self.inner.rpc_client.shutdown().await?)
    }

    /// A helper function that is not `async`, allowing connection
    /// process to be initiated from non-async contexts.
    pub fn connect_as_task(&self) -> Result<()> {
        let self_ = self.clone();
        workflow_core::task::spawn(async move {
            self_.inner.rpc_client.connect(ConnectOptions::default()).await.ok();
        });
        Ok(())
    }

    pub fn notification_channel_receiver(&self) -> Receiver<Notification> {
        self.inner.notification_channel.receiver.clone()
    }

    pub fn notification_mode(&self) -> NotificationMode {
        self.notification_mode
    }

    pub fn ctl(&self) -> &RpcCtl {
        &self.inner.rpc_ctl
    }

    pub fn parse_url_with_network_type(&self, url: String, network_type: NetworkType) -> Result<String> {
        Self::parse_url(url, self.inner.encoding, network_type)
    }

    pub fn parse_url(url: String, encoding: Encoding, network_type: NetworkType) -> Result<String> {
        let parse_output = parse_host(&url).map_err(|err| Error::Custom(err.to_string()))?;
        let scheme = parse_output
            .scheme
            .map(Ok)
            .unwrap_or_else(|| {
                if !application_runtime::is_web() {
                    return Ok("ws");
                }
                let location = window().location();
                let protocol =
                    location.protocol().map_err(|_| Error::UrlError("Unable to obtain window location protocol".to_string()))?;
                if protocol == "http:" || protocol == "chrome-extension:" {
                    Ok("ws")
                } else if protocol == "https:" {
                    Ok("wss")
                } else {
                    Err(Error::Custom(format!("Unsupported protocol: {}", protocol)))
                }
            })?
            .to_lowercase();
        let port = parse_output.port.unwrap_or_else(|| match encoding {
            WrpcEncoding::Borsh => network_type.default_borsh_rpc_port(),
            WrpcEncoding::SerdeJson => network_type.default_json_rpc_port(),
        });
        let path_str = parse_output.path;

        // Do not automatically include port if:
        //  1) the URL contains a scheme
        //  2) the URL contains a path
        //  3) explicitly specified in the URL,
        //
        //  This means wss://host.com or host.com/path will remain as-is
        //  while host.com or 1.2.3.4 will be converted to host.com:port
        //  or 1.2.3.4:port where port is based on the network type.
        //
        if (parse_output.scheme.is_some() || !path_str.is_empty()) && parse_output.port.is_none() {
            Ok(format!("{}://{}{}", scheme, parse_output.host.to_string(), path_str))
        } else {
            Ok(format!("{}://{}:{}{}", scheme, parse_output.host.to_string(), port, path_str))
        }
    }

    async fn start_rpc_ctl_service(&self) -> Result<()> {
        let inner = self.inner.clone();
        let wrpc_ctl_channel = inner.wrpc_ctl_multiplexer.channel();
        spawn(async move {
            loop {
                select! {
                    _ = inner.service_ctl.request.receiver.recv().fuse() => {
                        break;
                    },
                    msg = wrpc_ctl_channel.receiver.recv().fuse() => {
                        if let Ok(msg) = msg {
                            match msg {
                                WrpcCtl::Open => {
                                    inner.rpc_ctl.signal_open().await.expect("(KaspaRpcClient) rpc_ctl.signal_open() error");
                                }
                                WrpcCtl::Close => {
                                    inner.rpc_ctl.signal_close().await.expect("(KaspaRpcClient) rpc_ctl.signal_close() error");
                                }
                            }
                        } else {
                            log_error!("wrpc_ctl_channel.receiver.recv() error");
                        }
                    }
                }
            }
            inner.service_ctl.response.send(()).await.unwrap();
        });

        Ok(())
    }

    async fn stop_rpc_ctl_service(&self) -> Result<()> {
        self.inner.service_ctl.signal(()).await?;
        Ok(())
    }
}

#[async_trait]
impl RpcApi for KaspaRpcClient {
    //
    // The following proc-macro iterates over the array of enum variants
    // generating a function for each variant as follows:
    //
    // async fn ping_call(&self, request : PingRequest) -> RpcResult<PingResponse> {
    //     let response: ClientResult<PingResponse> = self.inner.rpc.call(RpcApiOps::Ping, request).await;
    //     Ok(response.map_err(|e| e.to_string())?)
    // }

    build_wrpc_client_interface!(
        RpcApiOps,
        [
            AddPeer,
            Ban,
            EstimateNetworkHashesPerSecond,
            GetBalanceByAddress,
            GetBalancesByAddresses,
            GetBlock,
            GetBlockCount,
            GetBlockDagInfo,
            GetBlocks,
            GetBlockTemplate,
            GetCoinSupply,
            GetConnectedPeerInfo,
            GetDaaScoreTimestampEstimate,
            GetServerInfo,
            GetCurrentNetwork,
            GetHeaders,
            GetInfo,
            GetMempoolEntries,
            GetMempoolEntriesByAddresses,
            GetMempoolEntry,
            GetPeerAddresses,
            GetMetrics,
            GetSink,
            GetSyncStatus,
            GetSubnetwork,
            GetUtxosByAddresses,
            GetSinkBlueScore,
            GetVirtualChainFromBlock,
            Ping,
            ResolveFinalityConflict,
            Shutdown,
            SubmitBlock,
            SubmitTransaction,
            Unban,
        ]
    );

    // ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
    // Notification API

    /// Register a new listener and returns an id and a channel receiver.
    fn register_new_listener(&self, connection: ChannelConnection) -> ListenerId {
        match self.notification_mode {
            NotificationMode::MultiListeners => self.notifier.as_ref().unwrap().register_new_listener(connection),
            NotificationMode::Direct => ListenerId::default(),
        }
    }

    /// Unregister an existing listener.
    ///
    /// Stop all notifications for this listener and drop its channel.
    async fn unregister_listener(&self, id: ListenerId) -> RpcResult<()> {
        match self.notification_mode {
            NotificationMode::MultiListeners => {
                self.notifier.as_ref().unwrap().unregister_listener(id)?;
            }
            NotificationMode::Direct => {}
        }
        Ok(())
    }

    /// Start sending notifications of some type to a listener.
    async fn start_notify(&self, id: ListenerId, scope: Scope) -> RpcResult<()> {
        match self.notification_mode {
            NotificationMode::MultiListeners => {
                self.notifier.clone().unwrap().try_start_notify(id, scope)?;
            }
            NotificationMode::Direct => {
                self.inner.start_notify_to_client(scope).await?;
            }
        }
        Ok(())
    }

    /// Stop sending notifications of some type to a listener.
    async fn stop_notify(&self, id: ListenerId, scope: Scope) -> RpcResult<()> {
        match self.notification_mode {
            NotificationMode::MultiListeners => {
                self.notifier.clone().unwrap().try_stop_notify(id, scope)?;
            }
            NotificationMode::Direct => {
                self.inner.stop_notify_to_client(scope).await?;
            }
        }
        Ok(())
    }
}
