use clap::{ArgGroup, Parser, Subcommand};
#[cfg(any(target_os = "linux", target_os = "windows"))]
use clap_num::maybe_hex;

#[cfg(any(target_os = "linux", target_os = "windows"))]
use crate::tuner::Voltage;

#[derive(Debug, Parser)]
#[clap(name = "recisdb-catv")]
#[clap(about = "recisdb-catv: ISDB-T / ISDB-C / TSMF receiver for TBS6281SE v2 & Linux chardev", long_about = None)]
#[clap(author = "maleicacid & contributors")]
#[clap(version)]
pub(crate) struct Cli {
    #[clap(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[clap(name = "checksignal")]
    Checksignal {
        #[clap(short = 'i', long, value_name = "CANONICAL_PATH", required = true)]
        device: String,

        #[clap(short, long, required = true)]
        channel: Option<String>,

        #[clap(short = 't', long = "type")]
        channel_type: Option<String>,

        #[clap(long = "tsmf-slot")]
        tsmf_slot: Option<i32>,

        #[clap(value_enum, long = "lnb")]
        lnb: Option<Voltage>,
    },

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[clap(group(
        ArgGroup::new("key")
            .args(&["key0", "key1"])
            .requires_all(&["key0", "key1"])
            .multiple(true)
    ))]
    Tune {
        #[clap(short = 'i', long, value_name = "CANONICAL_PATH", required = true)]
        device: Option<String>,

        #[clap(short, long, required = true)]
        channel: Option<String>,

        /// Channel type: GR (OFDM), CATV (QAM), BS, BS4K
        #[clap(short = 't', long = "type")]
        channel_type: Option<String>,

        /// TSMF relative slot number (1-15)
        #[clap(long = "tsmf-slot")]
        tsmf_slot: Option<i32>,

        /// Mirakurun scan flag
        #[clap(long = "scan")]
        scan: bool,

        #[clap(long)]
        card: Option<String>,

        #[clap(long, value_parser=maybe_hex::<u32>)]
        tsid: Option<u32>,

        #[clap(long, value_name = "seconds")]
        time: Option<f64>,

        #[clap(short = 'e', long)]
        exit_on_card_error: bool,

        #[clap(long = "no-decode")]
        no_decode: bool,

        #[clap(long = "no-simd")]
        no_simd: bool,

        #[clap(long = "no-strip")]
        no_strip: bool,

        #[clap(value_enum, long = "lnb")]
        lnb: Option<Voltage>,

        #[clap(long = "key0")]
        key0: Option<Vec<String>>,

        #[clap(long = "key1")]
        key1: Option<Vec<String>>,

        #[clap(required = true)]
        output: Option<String>,
    },

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    /// High-speed hybrid CATV & 4K sweep and channels.yml generator
    #[clap(name = "scan")]
    Scan {
        #[clap(short = 'i', long, value_name = "CANONICAL_PATH", required = true)]
        device: String,

        /// Output path for channels.yml
        #[clap(short, long)]
        output: Option<String>,

        /// Scan timeout per frequency in milliseconds
        #[clap(long, default_value = "800")]
        timeout_ms: u64,

        /// Output CATV channels as type CATV instead of CS
        #[clap(long = "catv", alias = "catv-type")]
        catv: bool,
    },

    Decode {
        #[clap(short = 'i', long = "input", value_name = "file", required = true)]
        source: Option<String>,

        #[clap(long = "no-simd")]
        no_simd: bool,

        #[clap(long = "no-strip")]
        no_strip: bool,

        #[clap(long)]
        card: Option<String>,

        #[clap(long = "key0")]
        key0: Option<Vec<String>>,

        #[clap(long = "key1")]
        key1: Option<Vec<String>>,

        #[clap(required = true)]
        output: Option<String>,
    },

    #[cfg(windows)]
    Enumerate {
        #[clap(short = 'i', long, value_name = "CANONICAL_PATH", required = true)]
        device: String,
        #[clap(short, long, required = true)]
        space: u32,
    },
}
