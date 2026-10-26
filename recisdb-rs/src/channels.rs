use crate::channels::representation::TsFilter;
pub(crate) use crate::channels::representation::{ChannelSpace, ChannelType};

pub mod output {
    use crate::channels::representation::{ChannelType, TsFilter};
    use log::error;

    #[repr(C)]
    #[allow(dead_code)]
    pub struct IoctlFreq {
        pub ch: i32,
        pub slot: i32,
    }

    impl From<ChannelType> for IoctlFreq {
        fn from(value: ChannelType) -> Self {
            match value {
                // 地上波: slot = 0 (純粋なISDB-T OFDM選局)
                ChannelType::Terrestrial(ch_num, ..) if (13..=62).contains(&ch_num) => {
                    Self {
                        ch: (ch_num as i32) + 50,
                        slot: 0,
                    }
                }
                // CATVおよびUHF帯ISDB-Cの全QAM自動追従スルー選局
                ChannelType::Catv(ch_num, ..)
                    if (13..=63).contains(&ch_num) || (113..=162).contains(&ch_num) =>
                {
                    let ch = if (113..=162).contains(&ch_num) {
                        (ch_num as i32) - 100 + 50
                    } else if (13..=22).contains(&ch_num) {
                        (ch_num as i32) - 10
                    } else {
                        (ch_num as i32) - 1
                    };
                    Self { ch, slot: -1 }
                }
                // 直接周波数指定によるISDB-C選局
                ChannelType::DirectFreq(freq_hz) => {
                    Self {
                        ch: 62,
                        slot: freq_hz as i32,
                    }
                }
                // BSスロット指定
                ChannelType::BS(ch_num, TsFilter::RelTsNum(slot))
                    if (1..=23).contains(&ch_num) && (ch_num % 2 == 1) =>
                {
                    Self {
                        ch: (ch_num as i32) / 2,
                        slot,
                    }
                }
                // BSスルー選局
                ChannelType::BS(ch_num, ..) if (1..=23).contains(&ch_num) && (ch_num % 2 == 1) => {
                    Self {
                        ch: (ch_num as i32) / 2,
                        slot: -1,
                    }
                }
                // CS選局
                ChannelType::CS(ch_num, TsFilter::AbsTsId(stream_id))
                    if (2..=24).contains(&ch_num) && (ch_num % 2 == 0) =>
                {
                    Self {
                        ch: (ch_num as i32) / 2 + 11,
                        slot: stream_id as i32,
                    }
                }
                ChannelType::CS(ch_num, ..) if (2..=24).contains(&ch_num) && (ch_num % 2 == 0) => {
                    Self {
                        ch: (ch_num as i32) / 2 + 11,
                        slot: 0,
                    }
                }
                ChannelType::BonCh(_) | ChannelType::BonChSpace(_) => {
                    error!("{:?} is not supported in Linux.", value);
                    std::process::exit(-1)
                }
                _ => {
                    error!("Invalid channel type: {:?}", value);
                    std::process::exit(-1)
                }
            }
        }
    }

    #[allow(dead_code)]
    pub struct DvbFreq {
        pub freq_hz: u32,
        pub stream_id: Option<u32>,
    }

    impl From<ChannelType> for DvbFreq {
        fn from(value: ChannelType) -> Self {
            let hz: u32 = match &value {
                ChannelType::Terrestrial(ch_num, ..) => {
                    let ch_num = *ch_num as u32;
                    473142857 + (ch_num - 13) * 6000000
                }
                ChannelType::Catv(ch_num, ..) => {
                    let ch_num = *ch_num as u32;
                    if (113..=162).contains(&ch_num) {
                        473000000 + (ch_num - 113) * 6000000
                    } else if (13..=21).contains(&ch_num) {
                        111142857 + (ch_num - 13) * 6000000
                    } else if ch_num == 22 {
                        167142857
                    } else if (23..=63).contains(&ch_num) {
                        225142857 + (ch_num - 23) * 6000000
                    } else {
                        473142857
                    }
                }
                ChannelType::DirectFreq(hz) => *hz,
                ChannelType::BS(ch_num, ..) => {
                    let ch = (*ch_num as i32) / 2;
                    (1049480 + (38360 * ch)) as u32
                }
                ChannelType::CS(ch_num, ..) => {
                    let ch = (*ch_num as i32) / 2 - 1;
                    (1613000 + (40000 * ch)) as u32
                }
                _ => 473142857,
            };

            let stream_id = match value {
                ChannelType::Catv(_, TsFilter::RelTsNum(id)) => Some(id as u32),
                ChannelType::BS(_, TsFilter::AbsTsId(id))
                | ChannelType::CS(_, TsFilter::AbsTsId(id)) => Some(id),
                ChannelType::BS(_, TsFilter::RelTsNum(id)) if id < 12 => Some(id as u32),
                _ => None,
            };

            Self {
                freq_hz: hz,
                stream_id,
            }
        }
    }
}

pub mod representation {
    use std::fmt::Display;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ChannelSpace {
        pub space: u32,
        pub ch: u32,
        pub space_description: Option<String>,
        pub ch_description: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum TsFilter {
        RelTsNum(i32),
        AbsTsId(u32),
        AsIs,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    #[allow(dead_code)]
    pub enum ChannelType {
        Terrestrial(u8, TsFilter),
        Catv(u8, TsFilter),
        BS(u8, TsFilter),
        CS(u8, TsFilter),
        DirectFreq(u32),
        BonCh(u8),
        BonChSpace(ChannelSpace),
        Undefined,
    }

    impl Display for ChannelType {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            type T = ChannelType;
            match self {
                T::Terrestrial(ch, TsFilter::AsIs) => write!(f, "Terrestrial: {ch} (OFDM)"),
                T::Terrestrial(ch, tsid) => write!(f, "Terrestrial: {ch} (OFDM, Filter={:?})", tsid),
                T::Catv(ch, TsFilter::AsIs) if *ch >= 113 => {
                    write!(f, "CATV UHF: {} (QAM Auto)", ch - 100)
                }
                T::Catv(ch, TsFilter::RelTsNum(slot)) if *ch >= 113 => {
                    write!(f, "CATV UHF: {} (TSMF Slot {slot})", ch - 100)
                }
                T::Catv(ch, TsFilter::AsIs) => write!(f, "CATV: C{ch} (QAM Auto)"),
                T::Catv(ch, TsFilter::RelTsNum(slot)) => write!(f, "CATV: C{ch} (TSMF Slot {slot})"),
                T::Catv(ch, tsid) => write!(f, "CATV: C{ch} (Filter={:?})", tsid),
                T::DirectFreq(hz) => write!(f, "DirectFreq: {hz} Hz (QAM)"),
                T::BS(ch, tsid) => write!(f, "BS: {ch}, {:?}", tsid),
                T::CS(ch, tsid) => write!(f, "CS: {ch}, {:?}", tsid),
                T::BonCh(ch) => write!(f, "BonDriver: Ch={ch}"),
                T::BonChSpace(ChannelSpace { ch, space, .. }) => {
                    write!(f, "BonDriver: Ch={ch}, Space={space}")
                }
                _ => write!(f, "Undefined"),
            }
        }
    }
}

pub struct Channel {
    pub ch_type: ChannelType,
    raw_string: String,
}

impl Channel {
    pub fn get_raw_ch_name(&self) -> &str {
        self.raw_string.as_str()
    }

    pub fn new_with_type(
        ch_str: impl Into<String>,
        type_opt: Option<&str>,
        tsmf_slot_opt: Option<i32>,
        override_stream_id: Option<u32>,
    ) -> Self {
        let raw_string = ch_str.into();
        let trimmed = raw_string.trim();

        // 1. 直接周波数指定判定 (例: "611000", "611MHz", "611000000")
        if let Some(hz) = parse_frequency(trimmed) {
            return Self {
                ch_type: ChannelType::DirectFreq(hz),
                raw_string,
            };
        }

        // 2. Type が指定されている場合の優先解釈
        if let Some(t) = type_opt {
            let t_upper = t.to_uppercase();
            if t_upper == "GR" {
                // 地上波強制: 数字のみまたは T付き
                let ch_num = extract_channel_number(trimmed).unwrap_or(0);
                if (13..=62).contains(&ch_num) {
                    return Self {
                        ch_type: ChannelType::Terrestrial(ch_num, TsFilter::AsIs),
                        raw_string,
                    };
                }
            } else if t_upper == "CATV" || t_upper == "BS" || t_upper == "BS4K" {
                let is_c_prefix = trimmed.starts_with('C') || trimmed.starts_with('c');
                let (ch_num, parsed_slot) = parse_catv_or_ch(trimmed);
                let slot = tsmf_slot_opt.or(parsed_slot);
                let ts_filter = match slot {
                    Some(s) if (1..=15).contains(&s) => TsFilter::RelTsNum(s),
                    _ => TsFilter::AsIs,
                };
                if is_c_prefix && (13..=63).contains(&ch_num) {
                    return Self {
                        ch_type: ChannelType::Catv(ch_num, ts_filter),
                        raw_string,
                    };
                } else if !is_c_prefix && (13..=62).contains(&ch_num) {
                    // UHF帯のISDB-C搬送波を選局する
                    return Self {
                        ch_type: ChannelType::Catv(ch_num + 100, ts_filter),
                        raw_string,
                    };
                }
            }
        }

        // 3. 通常の文字列パース
        let upper = trimmed.to_uppercase();

        // 地上波 (例: "T25", "GR25", "GR_25")
        if upper.starts_with("GR") || upper.starts_with('T') {
            if let Some(num) = extract_channel_number(&upper) {
                if (13..=62).contains(&num) {
                    return Self {
                        ch_type: ChannelType::Terrestrial(num, TsFilter::AsIs),
                        raw_string,
                    };
                }
            }
        }

        // CATV (例: "C49", "C49_1", "C49:1", "C49-1")
        if upper.starts_with('C') && !upper.starts_with("CS") {
            let (num, parsed_slot) = parse_catv_or_ch(&upper);
            let slot = tsmf_slot_opt.or(parsed_slot);
            let ts_filter = match slot {
                Some(s) if (1..=15).contains(&s) => TsFilter::RelTsNum(s),
                _ => TsFilter::AsIs,
            };
            if (13..=63).contains(&num) {
                return Self {
                    ch_type: ChannelType::Catv(num, ts_filter),
                    raw_string,
                };
            }
        }

        // UHF帯CATV選局のプレフィックス解釈
        if upper.starts_with('U') {
            let (num, parsed_slot) = parse_catv_or_ch(&upper);
            let slot = tsmf_slot_opt.or(parsed_slot);
            let ts_filter = match slot {
                Some(s) if (1..=15).contains(&s) => TsFilter::RelTsNum(s),
                _ => TsFilter::AsIs,
            };
            if (13..=62).contains(&num) {
                return Self {
                    ch_type: ChannelType::Catv(num + 100, ts_filter),
                    raw_string,
                };
            }
        }

        // BS (例: "BS1_0", "BS03_1")
        if upper.starts_with("BS") {
            let rest = upper.trim_start_matches("BS").trim_start_matches('_');
            if let Some((first, second)) = parse_pair(rest) {
                let ts_filter = match override_stream_id {
                    Some(id) => TsFilter::AbsTsId(id),
                    None => TsFilter::RelTsNum(second as i32),
                };
                return Self {
                    ch_type: ChannelType::BS(first as u8, ts_filter),
                    raw_string,
                };
            } else if let Ok(first) = rest.parse::<u8>() {
                let ts_filter = match override_stream_id {
                    Some(id) => TsFilter::AbsTsId(id),
                    None => TsFilter::AsIs,
                };
                return Self {
                    ch_type: ChannelType::BS(first, ts_filter),
                    raw_string,
                };
            }
        }

        // CS (例: "CS02", "CS24")
        if upper.starts_with("CS") {
            let rest = upper.trim_start_matches("CS").trim_start_matches('_');
            if let Ok(first) = rest.parse::<u8>() {
                let ts_filter = match override_stream_id {
                    Some(id) => TsFilter::AbsTsId(id),
                    None => TsFilter::AsIs,
                };
                return Self {
                    ch_type: ChannelType::CS(first, ts_filter),
                    raw_string,
                };
            }
        }

        // 数字のみ (例: "25", "27") -> Linuxでは地上波13〜62chを最優先
        if let Ok(num) = trimmed.parse::<u8>() {
            if (13..=62).contains(&num) {
                return Self {
                    ch_type: ChannelType::Terrestrial(num, TsFilter::AsIs),
                    raw_string,
                };
            }
        }

        // それ以外
        Self {
            ch_type: ChannelType::Undefined,
            raw_string,
        }
    }

    #[allow(dead_code)]
    pub fn new(ch_str: impl Into<String>, override_stream_id: Option<u32>) -> Self {
        Self::new_with_type(ch_str, None, None, override_stream_id)
    }
}

fn parse_frequency(s: &str) -> Option<u32> {
    let lower = s.to_lowercase();
    if lower.ends_with("mhz") {
        let num_str = lower.trim_end_matches("mhz").trim();
        if let Ok(mhz) = num_str.parse::<f64>() {
            return Some((mhz * 1_000_000.0) as u32);
        }
    } else if lower.ends_with("khz") {
        let num_str = lower.trim_end_matches("khz").trim();
        if let Ok(khz) = num_str.parse::<f64>() {
            return Some((khz * 1_000.0) as u32);
        }
    } else if let Ok(val) = s.parse::<u32>() {
        if (90_000..=800_000).contains(&val) {
            // kHz指定
            return Some(val * 1000);
        } else if (90_000_000..=800_000_000).contains(&val) {
            // Hz指定
            return Some(val);
        }
    }
    None
}

fn extract_channel_number(s: &str) -> Option<u8> {
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse::<u8>().ok()
}

fn parse_catv_or_ch(s: &str) -> (u8, Option<i32>) {
    let stripped = s.trim_start_matches(|c: char| !c.is_ascii_digit());
    if let Some((first, second)) = parse_pair(stripped) {
        (first as u8, Some(second as i32))
    } else if let Ok(num) = stripped.parse::<u8>() {
        (num, None)
    } else {
        (0, None)
    }
}

fn parse_pair(s: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = s.split(|c| c == '_' || c == ':' || c == '-').collect();
    if parts.len() == 2 {
        if let (Ok(a), Ok(b)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
            return Some((a, b));
        }
    }
    None
}
