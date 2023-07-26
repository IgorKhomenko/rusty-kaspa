use std::pin::Pin;

use futures::future::join_all;
use workflow_core::task::sleep;

use crate::imports::*;
use kaspa_rpc_core::{api::rpc::RpcApi, GetMetricsResponse};

// use kaspa_rpc_core::{ConsensusMetrics, ProcessMetrics};
// use workflow_nw::ipc::*;
// use kaspa_metrics::{MetricsCtl, data::MetricsData, result::Result as MetricsResult};
use super::MetricsData;

// pub type MetricsSinkFn = Arc<Box<(dyn Fn(MetricsData))>>;
pub type MetricsSinkFn =
    Arc<Box<dyn Send + Sync + Fn(MetricsData) -> Pin<Box<(dyn Send + 'static + Future<Output = Result<()>>)>> + 'static>>;

#[derive(Describe, Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum MetricsSettings {
    #[describe("Mute logs")]
    Mute,
}

#[async_trait]
impl DefaultSettings for MetricsSettings {
    async fn defaults() -> Vec<(Self, Value)> {
        // let mut settings = vec![(Self::Mute, "false".to_string())];
        // settings
        vec![]
    }
}

pub struct Metrics {
    settings: SettingsStore<MetricsSettings>,
    mute: Arc<AtomicBool>,
    task_ctl: DuplexChannel,
    rpc: Arc<Mutex<Option<Arc<dyn RpcApi>>>>,
    // target : Arc<Mutex<Option<Arc<dyn MetricsCtl>>>>,
    sink: Arc<Mutex<Option<MetricsSinkFn>>>,
    data: Arc<Mutex<MetricsData>>,
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics {
            settings: SettingsStore::try_new("metrics").expect("Failed to create miner settings store"),
            mute: Arc::new(AtomicBool::new(true)),
            task_ctl: DuplexChannel::oneshot(),
            rpc: Arc::new(Mutex::new(None)),
            sink: Arc::new(Mutex::new(None)),
            data: Arc::new(Mutex::new(MetricsData::default())),
        }
    }
}

#[async_trait]
impl Handler for Metrics {
    fn verb(&self, _ctx: &Arc<dyn Context>) -> Option<&'static str> {
        Some("metrics")
    }

    fn help(&self, _ctx: &Arc<dyn Context>) -> &'static str {
        "Manage metrics monitoring"
    }

    async fn start(self: Arc<Self>, _ctx: &Arc<dyn Context>) -> cli::Result<()> {
        self.settings.try_load().await.ok();
        if let Some(mute) = self.settings.get(MetricsSettings::Mute) {
            self.mute.store(mute, Ordering::Relaxed);
        }

        self.start_task().await?;
        Ok(())
    }

    async fn stop(self: Arc<Self>, _ctx: &Arc<dyn Context>) -> cli::Result<()> {
        self.stop_task().await?;
        Ok(())
    }

    async fn handle(self: Arc<Self>, ctx: &Arc<dyn Context>, argv: Vec<String>, cmd: &str) -> cli::Result<()> {
        let ctx = ctx.clone().downcast_arc::<KaspaCli>()?;
        self.main(ctx, argv, cmd).await.map_err(|e| e.into())
    }
}

impl Metrics {
    fn rpc(&self) -> Option<Arc<dyn RpcApi>> {
        self.rpc.lock().unwrap().clone()
    }

    pub fn register_sink(&self, target: MetricsSinkFn) {
        self.sink.lock().unwrap().replace(target);
    }

    pub fn unregister_sink(&self) {
        self.sink.lock().unwrap().take();
    }

    pub fn sink(&self) -> Option<MetricsSinkFn> {
        self.sink.lock().unwrap().clone()
    }

    async fn main(self: Arc<Self>, ctx: Arc<KaspaCli>, mut argv: Vec<String>, _cmd: &str) -> Result<()> {
        if argv.is_empty() {
            return self.display_help(ctx, argv).await;
        }
        match argv.remove(0).as_str() {
            "open" => {}
            v => {
                tprintln!(ctx, "unknown command: '{v}'\r\n");

                return self.display_help(ctx, argv).await;
            }
        }

        Ok(())
    }

    pub async fn start_task(self: &Arc<Self>) -> Result<()> {
        let this = self.clone();

        let task_ctl_receiver = self.task_ctl.request.receiver.clone();
        let task_ctl_sender = self.task_ctl.response.sender.clone();

        spawn(async move {
            loop {
                let poll = sleep(Duration::from_millis(1000));

                select! {
                    _ = task_ctl_receiver.recv().fuse() => {
                        break;
                    },
                    _ = poll.fuse() => {

                        *this.data.lock().unwrap() = MetricsData::new(unixtime_as_millis_f64());

                        if let Some(rpc) = this.rpc() {
                            let samples = vec![
                                this.sample_metrics(rpc.clone()).boxed(),
                                this.sample_gbdi(rpc.clone()).boxed(),
                                this.sample_cpi(rpc.clone()).boxed(),
                            ];

                            join_all(samples).await;
                        }

                        // TODO - output to terminal...
                        if let Some(sink) = this.sink() {
                            let data = this.data.lock().unwrap().clone();
                            sink(data).await.ok();
                        }
                    }
                }
            }

            task_ctl_sender.send(()).await.unwrap();
        });
        Ok(())
    }

    pub async fn stop_task(&self) -> Result<()> {
        self.task_ctl.signal(()).await.expect("Metrics::stop_task() signal error");
        Ok(())
    }

    pub async fn display_help(self: &Arc<Self>, ctx: Arc<KaspaCli>, _argv: Vec<String>) -> Result<()> {
        let help = "\n\
            \topen  - Open metrics window\n\
            \tclose - Close metrics window\n\
        \n\
        ";

        tprintln!(ctx, "{}", help.crlf());

        Ok(())
    }

    // --- samplers

    async fn sample_metrics(self: &Arc<Self>, rpc: Arc<dyn RpcApi>) -> Result<()> {
        if let Ok(metrics) = rpc.get_metrics(true, true).await {
            #[allow(unused_variables)]
            let GetMetricsResponse { server_time, consensus_metrics, process_metrics } = metrics;

            let mut data = self.data.lock().unwrap();
            if let Some(consensus_metrics) = consensus_metrics {
                data.blocks_submitted = consensus_metrics.blocks_submitted;
                data.header_counts = consensus_metrics.header_counts;
                data.dep_counts = consensus_metrics.dep_counts;
                data.body_counts = consensus_metrics.body_counts;
                data.txs_counts = consensus_metrics.txs_counts;
                data.chain_block_counts = consensus_metrics.chain_block_counts;
                data.mass_counts = consensus_metrics.mass_counts;
            }
        }

        Ok(())
    }

    async fn sample_gbdi(self: &Arc<Self>, rpc: Arc<dyn RpcApi>) -> Result<()> {
        if let Ok(gdbi) = rpc.get_block_dag_info().await {
            let mut data = self.data.lock().unwrap();
            data.block_count = gdbi.block_count;
            // data.header_count = gdbi.header_count;
            data.tip_hashes = gdbi.tip_hashes.len();
            data.difficulty = gdbi.difficulty;
            data.past_median_time = gdbi.past_median_time;
            data.virtual_parent_hashes = gdbi.virtual_parent_hashes.len();
            data.virtual_daa_score = gdbi.virtual_daa_score;
        }

        Ok(())
    }

    async fn sample_cpi(self: &Arc<Self>, rpc: Arc<dyn RpcApi>) -> Result<()> {
        if let Ok(_cpi) = rpc.get_connected_peer_info().await {
            // let mut data = self.data.lock().unwrap();
            // - TODO - fold peers into inbound / outbound...
        }

        Ok(())
    }
}