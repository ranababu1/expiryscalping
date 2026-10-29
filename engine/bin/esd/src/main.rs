//! `esd`: the ExpiryScalping engine.
//!
//! ```text
//! esd sim                      simulated market, paper trading, API on
//! esd sim --speed 0 --headless whole simulated day as fast as possible
//! esd replay data/raw/x.eslog  replay a recorded raw tick log
//! esd device add laptop        new device token for the desktop app
//! ```

use clap::{Parser, Subcommand};
use es_api::{DeviceStore, Publisher};
use es_core::Date;
use esd::config::{ConfigMode, EngineConfig};
use esd::{Engine, EngineOptions, Journal, RunOptions};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info};

#[derive(Parser)]
#[command(
    name = "esd",
    version,
    about = "ExpiryScalping engine (Phase 1: paper only)"
)]
struct Cli {
    /// Engine config. Default: config/engine.toml, then engine/config/engine.toml.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Risk limits. Default: risk.toml next to the engine config.
    #[arg(long, global = true)]
    risk: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Paper trade on the built-in market simulator.
    Sim {
        /// Trading day to simulate (YYYY-MM-DD). Default: today in IST.
        #[arg(long)]
        date: Option<String>,
        /// 1 = real time, 10 = ten times faster, 0 = as fast as possible.
        #[arg(long, default_value_t = 10.0)]
        speed: f64,
        /// Also write the ticks to this raw log, for replay later.
        #[arg(long)]
        record: Option<PathBuf>,
        /// No client API; print a summary and exit when the day ends.
        #[arg(long)]
        headless: bool,
    },
    /// Paper trade on a recorded raw tick log.
    Replay {
        file: PathBuf,
        #[arg(long, default_value_t = 1.0)]
        speed: f64,
        #[arg(long)]
        headless: bool,
    },
    /// Manage desktop app device tokens.
    Device {
        #[command(subcommand)]
        cmd: DeviceCmd,
    },
}

#[derive(Subcommand)]
enum DeviceCmd {
    /// Create a device and print its token once.
    Add {
        name: String,
    },
    List,
    Revoke {
        name: String,
    },
}

fn find_config(given: Option<PathBuf>) -> PathBuf {
    if let Some(p) = given {
        return p;
    }
    for c in ["config/engine.toml", "engine/config/engine.toml"] {
        let p = PathBuf::from(c);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("config/engine.toml")
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    match real_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("{e}");
            eprintln!("esd: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn real_main() -> Result<(), String> {
    let cli = Cli::parse();
    let cfg_path = find_config(cli.config);
    let cfg = EngineConfig::load(&cfg_path).map_err(|e| e.to_string())?;
    if cfg.mode == ConfigMode::Live {
        return Err("mode = \"live\" is not available in Phase 1. Set mode = \"paper\".".into());
    }
    let risk_path = cli.risk.unwrap_or_else(|| {
        cfg_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("risk.toml")
    });
    let devices_path = cfg.data_dir.join("devices.json");

    match cli.cmd {
        Cmd::Device { cmd } => device_cmd(&devices_path, cmd),
        Cmd::Sim {
            date,
            speed,
            record,
            headless,
        } => {
            let date = match date {
                Some(d) => Date::parse(&d).map_err(|e| e.to_string())?,
                None => esd::today_ist(),
            };
            let sim = es_replay::Simulator::new(&cfg.sim, date).map_err(|e| e.to_string())?;
            let source = format!("simulator {date} x{speed}");
            let (tx, rx) = mpsc::channel(4096);
            tokio::spawn(es_replay::pace(sim, speed, tx));
            let recorder = match record {
                Some(p) => Some(esd::spawn_recorder(&p).map_err(|e| e.to_string())?),
                None => None,
            };
            serve(
                cfg,
                &risk_path,
                &devices_path,
                source,
                rx,
                recorder,
                headless,
            )
            .await
        }
        Cmd::Replay {
            file,
            speed,
            headless,
        } => {
            let reader = es_replay::open_replay(&file).map_err(|e| e.to_string())?;
            let source = format!("replay {} x{speed}", file.display());
            let (tx, rx) = mpsc::channel(4096);
            let records = reader.map_while(|r| match r {
                Ok(r) => Some(r),
                Err(e) => {
                    error!(%e, "replay: log unreadable from here, stopping");
                    None
                }
            });
            tokio::spawn(es_replay::pace(records, speed, tx));
            serve(cfg, &risk_path, &devices_path, source, rx, None, headless).await
        }
    }
}

fn device_cmd(path: &Path, cmd: DeviceCmd) -> Result<(), String> {
    let mut store = DeviceStore::load(path).map_err(|e| e.to_string())?;
    match cmd {
        DeviceCmd::Add { name } => {
            let token = store
                .add(&name, esd::wall_ms())
                .map_err(|e| e.to_string())?;
            store.save().map_err(|e| e.to_string())?;
            println!("Device {name:?} added. Its token (shown once, paste it into the app):");
            println!("{token}");
        }
        DeviceCmd::List => {
            if store.devices.is_empty() {
                println!("No devices.");
            }
            for d in &store.devices {
                println!("{}", d.name);
            }
        }
        DeviceCmd::Revoke { name } => {
            store.revoke(&name).map_err(|e| e.to_string())?;
            store.save().map_err(|e| e.to_string())?;
            println!("Device {name:?} revoked.");
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn serve(
    cfg: EngineConfig,
    risk_path: &Path,
    devices_path: &Path,
    source: String,
    records: mpsc::Receiver<es_feed::RawRecord>,
    recorder: Option<esd::Recorder>,
    headless: bool,
) -> Result<(), String> {
    let (limits, from) = esd::load_risk(risk_path, &cfg.data_dir).map_err(|e| e.to_string())?;
    info!(from = %from.display(), "risk limits loaded");
    let version = env!("CARGO_PKG_VERSION").to_string();
    let day = esd::today_ist();
    let journal_path = cfg
        .data_dir
        .join("journal")
        .join(format!("{day}-{}.jsonl", esd::wall_ms()));
    let journal = Journal::to_file(&journal_path).map_err(|e| e.to_string())?;
    let publish_every = Duration::from_millis(cfg.timing.publish_ms);
    let bind = cfg.api_bind.clone();
    let publisher = (!headless).then(Publisher::new);
    let opts = EngineOptions {
        source,
        version: version.clone(),
        data_dir: Some(cfg.data_dir.clone()),
    };
    let engine =
        Engine::new(cfg, limits, opts, journal, publisher.clone()).map_err(|e| e.to_string())?;
    let (cmd_tx, cmd_rx) = mpsc::channel(256);

    if let Some(publisher) = publisher {
        let mut store = DeviceStore::load(devices_path).map_err(|e| e.to_string())?;
        if store.devices.is_empty() {
            let token = store
                .add("desktop", esd::wall_ms())
                .map_err(|e| e.to_string())?;
            store.save().map_err(|e| e.to_string())?;
            println!("First run: created device \"desktop\". Paste this token into the app.");
            println!("It is shown only once (add more with `esd device add <name>`):");
            println!("{token}");
        }
        let listener = tokio::net::TcpListener::bind(&bind)
            .await
            .map_err(|e| format!("cannot listen on {bind}: {e}"))?;
        info!(%bind, "client API listening");
        tokio::spawn(es_api::serve(
            listener,
            Arc::new(RwLock::new(store)),
            cmd_tx,
            publisher,
            version,
        ));
    } else {
        drop(cmd_tx);
    }

    let (recorder, recorder_thread) = match recorder {
        Some((tx, h)) => (Some(tx), Some(h)),
        None => (None, None),
    };
    let stop = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let engine = esd::run(
        engine,
        records,
        cmd_rx,
        RunOptions {
            publish_every,
            recorder,
            exit_when_done: headless,
        },
        stop,
    )
    .await;
    if let Some(h) = recorder_thread
        && h.join().is_err()
    {
        error!("recorder thread panicked");
    }
    if headless {
        print_summary(&engine);
    }
    info!(journal = %journal_path.display(), "journal written");
    Ok(())
}

fn print_summary(engine: &Engine) {
    let p = engine.positions_dto();
    let o = engine.orders_dto();
    let fills = o.orders.iter().filter(|x| x.filled > 0).count();
    let rs = |paise: i64| format!("Rs {:.2}", paise as f64 / 100.0);
    println!("Session summary (paper):");
    println!("  orders: {}, with fills: {fills}", o.orders.len());
    println!(
        "  realised: {}, unrealised: {}, charges: {}",
        rs(p.pnl.realised),
        rs(p.pnl.unrealised),
        rs(p.pnl.charges)
    );
    println!("  funds: {}", rs(p.pnl.available_funds));
}
