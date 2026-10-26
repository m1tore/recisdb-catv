use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};

use log::info;

use crate::channels::output::IoctlFreq;

nix::ioctl_write_ptr!(set_ch, 0x8d, 0x01, IoctlFreq);
nix::ioctl_none!(start_rec, 0x8d, 0x02);
nix::ioctl_none!(stop_rec, 0x8d, 0x03);
nix::ioctl_read!(ptx_get_cnr, 0x8d, 0x04, i64);

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ScannedChannel {
    pub name: String,
    pub channel_str: String,
    pub channel_type: String,
    pub service_id: Option<u16>,
    pub service_type: Option<u8>,
    pub tsmf_slot: Option<u8>,
    pub freq_mhz: f64,
    pub modulation: String,
    pub is_4k: bool,
}

#[derive(Clone, Debug)]
struct CnitTsEntry {
    freq_key: u32,
    frame_type: u8,
    services: Vec<(u16, u8)>,
    is_actual: bool,
}

pub fn run_scan(device_path: String, output_path: Option<String>, timeout_ms: u64, use_catv_type: bool) {
    info!("Starting C-NIT based CATV & BS scan on {}", device_path);
    println!("High-speed C-NIT based CATV & BS scan on {}...", device_path);

    // ノンブロッキングモードでデバイスを開く
    let file = match File::options()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&device_path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open tuner device {}: {}", device_path, e);
            std::process::exit(1);
        }
    };
    let fd = file.as_raw_fd();

    // 周波数グリッドを構築する
    let mut target_freqs: Vec<(i32, i32, f64, String)> = Vec::new();

    // C13からC22の周波数グリッド
    for ch in 13..=22 {
        let freq_mhz = 111.0 + (ch - 13) as f64 * 6.0;
        let ioctl_ch = ch - 10;
        target_freqs.push((ioctl_ch, -1, freq_mhz, format!("C{}", ch)));
    }
    // C23からC63の周波数グリッド
    for ch in 23..=63 {
        let freq_mhz = 225.0 + (ch - 23) as f64 * 6.0;
        let ioctl_ch = ch - 1;
        target_freqs.push((ioctl_ch, -1, freq_mhz, format!("C{}", ch)));
    }
    // UHF帯13chから62chのISDB-C周波数グリッド
    for ch in 13..=62 {
        let freq_mhz = 473.0 + (ch - 13) as f64 * 6.0;
        let ioctl_ch = ch + 50;
        target_freqs.push((ioctl_ch, -1, freq_mhz, format!("{}", ch)));
    }

    let mut physically_locked_freqs: HashSet<u32> = HashSet::new();
    let mut cnit_entries: BTreeMap<(u16, u16), CnitTsEntry> = BTreeMap::new();
    let mut sdt_services: HashMap<(u16, u16, u16), (String, u8, bool)> = HashMap::new();
    let mut tsmf_slots_by_freq: HashMap<u32, Vec<(u8, u16, u16)>> = HashMap::new();
    let locked_timeout_ms = timeout_ms.max(2200);

    // 全周波数のスイープを実行する
    let mut buf = [0u8; 188 * 50];
    for (ch, slot, freq_mhz, label) in target_freqs {
        print!("Probing {:<8} ({:6.1} MHz)... ", label, freq_mhz);
        std::io::stdout().flush().unwrap();

        let freq_key = (freq_mhz * 10.0).round() as u32;

        let ioctl_arg = IoctlFreq { ch, slot };
        if unsafe { set_ch(fd, &ioctl_arg) }.is_err() {
            println!("ioctl failed");
            continue;
        }

        let _ = unsafe { start_rec(fd) };

        // チューナー整定を待機する
        std::thread::sleep(Duration::from_millis(60));

        let start_time = Instant::now();
        let mut got_stream = false;
        let mut nit_found = false;
        let mut actual_sdt_slots: HashSet<usize> = HashSet::new();
        let sdt_count_before = sdt_services.len();
        let mut nit_bufs: [Vec<u8>; 16] = std::array::from_fn(|_| Vec::with_capacity(4096));
        let mut sdt_bufs: [Vec<u8>; 16] = std::array::from_fn(|_| Vec::with_capacity(4096));
        let mut local_tsmf_slots: Vec<(u8, u16, u16)> = Vec::new();
        let mut tsmf_slot_map = [0u8; 52];
        let mut tsmf_pkt_idx: i32 = -1;

        while start_time.elapsed() < Duration::from_millis(locked_timeout_ms) {
            let mut poll_fd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ret = unsafe { libc::poll(&mut poll_fd, 1, 50) };
            if ret > 0 && (poll_fd.revents & libc::POLLIN) != 0 {
                if let Ok(n) = nix::unistd::read(fd, &mut buf) {
                    if n >= 188 {
                        got_stream = true;
                        for chunk in buf[..n].chunks_exact(188) {
                            if chunk[0] != 0x47 {
                                continue;
                            }
                            let pid = (((chunk[1] & 0x1F) as u16) << 8) | (chunk[2] as u16);
                            if pid == 0x002F {
                                if parse_tsmf_packet(
                                    chunk,
                                    &mut local_tsmf_slots,
                                    &mut tsmf_slot_map,
                                ) {
                                    tsmf_pkt_idx = 0;
                                    continue;
                                }
                            }

                            let cur_slot = if (0..52).contains(&tsmf_pkt_idx) {
                                let s = tsmf_slot_map[tsmf_pkt_idx as usize] as usize;
                                tsmf_pkt_idx += 1;
                                if s == 0 || s >= 16 {
                                    continue;
                                }
                                s
                            } else {
                                0
                            };

                            if pid == 0x0010 {
                                if accumulate_psi_packet(chunk, &mut nit_bufs[cur_slot], |sec| {
                                    parse_nit_section(sec, &mut cnit_entries);
                                }) {
                                    nit_found = true;
                                }
                            } else if pid == 0x0011 {
                                accumulate_psi_packet(chunk, &mut sdt_bufs[cur_slot], |sec| {
                                    if parse_sdt_section(sec, &mut sdt_services) {
                                        actual_sdt_slots.insert(cur_slot);
                                    }
                                });
                            }
                        }
                    }
                }
            } else if ret == 0 && !got_stream && start_time.elapsed() >= Duration::from_millis(120) {
                // 信号未検出時は早期離脱する
                break;
            }

            let expected_slots = local_tsmf_slots
                .iter()
                .filter(|&&(_, _, s_onid)| s_onid != 0xFFFF)
                .count()
                .max(1);
            if nit_found
                && actual_sdt_slots.len() >= expected_slots
                && start_time.elapsed() >= Duration::from_millis(250)
            {
                break;
            }
        }

        let _ = unsafe { stop_rec(fd) };

        if got_stream {
            physically_locked_freqs.insert(freq_key);
            if !local_tsmf_slots.is_empty() {
                tsmf_slots_by_freq.insert(freq_key, local_tsmf_slots);
            }
            let new_services = sdt_services.len().saturating_sub(sdt_count_before);
            println!("LOCKED! (New SDT entries: {}, Total: {})", new_services, sdt_services.len());
        } else {
            println!("no signal");
        }
    }

    // C-NITの定義に基づいてBSおよびCATVサービスを構築する
    println!("\nBuilding channel list strictly from C-NIT and SDT...");
    let mut scanned_services: Vec<ScannedChannel> = Vec::new();
    let mut cnit_registered_freqs: HashSet<u32> = HashSet::new();

    for ((onid, tsid), entry) in &cnit_entries {
        if entry.freq_key > 0 {
            cnit_registered_freqs.insert(entry.freq_key);
        }

        // 物理ロックが確認できた搬送波のみを対象とする
        if !physically_locked_freqs.contains(&entry.freq_key) {
            continue;
        }

        // C-NITのオリジナルネットワークIDに基づいて放送種別を決定する
        let channel_type = match *onid {
            0x0004 => "BS".to_string(),
            0x000B | 0x000C => "BS4K".to_string(),
            0xFFFE | 0xFFFA | 0xFFFD => {
                if use_catv_type {
                    "CATV".to_string()
                } else {
                    "CS".to_string()
                }
            }
            _ => continue,
        };

        let base_ch_str = match freq_key_to_channel_str(entry.freq_key) {
            Some(s) => s,
            None => continue,
        };

        // 同一搬送波に複数の有効なTSMFスロットがある場合はスロット番号を付与する
        let mut tsmf_slot_opt: Option<u8> = None;
        if entry.frame_type == 1 {
            if let Some(slots) = tsmf_slots_by_freq.get(&entry.freq_key) {
                let valid_slots: Vec<&(u8, u16, u16)> = slots
                    .iter()
                    .filter(|&&(_, _, s_onid)| s_onid != 0xFFFF)
                    .collect();
                if valid_slots.len() > 1 {
                    for &&(s_num, s_tsid, s_onid) in &valid_slots {
                        if s_tsid == *tsid && s_onid == *onid {
                            tsmf_slot_opt = Some(s_num);
                            break;
                        }
                    }
                }
            }
        }

        let channel_str = match tsmf_slot_opt {
            Some(s) => format!("{}_{}", base_ch_str, s),
            None => base_ch_str,
        };

        let freq_mhz = (entry.freq_key as f64) / 10.0;

        // C-NITのサービスリスト記述子またはSDTからサービス一覧を抽出する
        let mut service_ids: Vec<(u16, u8)> = entry.services.clone();
        if service_ids.is_empty() {
            for (&(s_onid, s_tsid, sid), &(_, stype, _)) in &sdt_services {
                if s_onid == *onid && s_tsid == *tsid {
                    service_ids.push((sid, stype));
                }
            }
            service_ids.sort_by_key(|&(sid, _)| sid);
        }

        for (sid, nit_stype) in service_ids {
            let (name, stype) = if let Some((sname, s_type, _)) = sdt_services.get(&(*onid, *tsid, sid)) {
                (sname.clone(), *s_type)
            } else {
                continue;
            };

            // テレビおよび音声・4K放送サービスのみを出力対象とする
            if stype != 0x01 && stype != 0x02 && stype != 0xA1 && stype != 0xA2 && stype != 0xAD {
                continue;
            }

            scanned_services.push(ScannedChannel {
                name,
                channel_str: channel_str.clone(),
                channel_type: channel_type.clone(),
                service_id: Some(sid),
                service_type: Some(stype.max(nit_stype)),
                tsmf_slot: tsmf_slot_opt,
                freq_mhz,
                modulation: "QAM".to_string(),
                is_4k: channel_type == "BS4K",
            });
        }
    }

    // 放送種別とサービスIDの順でソートする
    scanned_services.sort_by(|a, b| {
        let type_order = |t: &str| match t {
            "GR" => 0,
            "BS" => 1,
            "BS4K" => 2,
            "CS" => 3,
            "CATV" => 4,
            _ => 5,
        };
        type_order(&a.channel_type)
            .cmp(&type_order(&b.channel_type))
            .then_with(|| a.service_id.cmp(&b.service_id))
    });

    // YAMLファイルを生成する
    let yaml_output = generate_channels_yaml(&scanned_services);
    if let Some(ref path) = output_path {
        if let Ok(mut f) = File::create(path) {
            let _ = f.write_all(yaml_output.as_bytes());
            println!("\nGenerated channels.yml successfully saved to {}", path);
        } else {
            eprintln!("Failed to write to {}", path);
        }
    } else {
        println!("\n=== Generated channels.yml ===");
        println!("{}", yaml_output);
    }
}

fn freq_key_to_channel_str(freq_key: u32) -> Option<String> {
    let mhz = freq_key / 10;
    if (111..=165).contains(&mhz) && (mhz - 111) % 6 == 0 {
        Some(format!("C{}", 13 + (mhz - 111) / 6))
    } else if mhz == 167 {
        Some("C22".to_string())
    } else if (225..=465).contains(&mhz) && (mhz - 225) % 6 == 0 {
        Some(format!("C{}", 23 + (mhz - 225) / 6))
    } else if (473..=767).contains(&mhz) && (mhz - 473) % 6 == 0 {
        Some(format!("{}", 13 + (mhz - 473) / 6))
    } else if mhz >= 90 {
        Some(format!("{}MHz", mhz))
    } else {
        None
    }
}

fn parse_tsmf_packet(
    pkt: &[u8],
    slots: &mut Vec<(u8, u16, u16)>,
    slot_map: &mut [u8; 52],
) -> bool {
    if pkt.len() < 188 || pkt[0] != 0x47 {
        return false;
    }
    let frame_sync = (((pkt[4] & 0x1F) as u16) << 8) | (pkt[5] as u16);
    if frame_sync != 0x1A86 && frame_sync != 0x0579 {
        return false;
    }
    if slots.is_empty() {
        for s in 0..15usize {
            let off = 9 + s * 4;
            if off + 4 <= 188 {
                let tsid = ((pkt[off] as u16) << 8) | (pkt[off + 1] as u16);
                let onid = ((pkt[off + 2] as u16) << 8) | (pkt[off + 3] as u16);
                if tsid != 0x0000 && tsid != 0xFFFF {
                    slots.push(((s + 1) as u8, tsid, onid));
                }
            }
        }
    }
    for k in 0..52usize {
        let b = pkt[73 + k / 2];
        slot_map[k] = if k % 2 == 0 { b >> 4 } else { b & 0x0F };
    }
    true
}

fn accumulate_psi_packet<F>(pkt: &[u8], psi_buf: &mut Vec<u8>, mut on_section: F) -> bool
where
    F: FnMut(&[u8]),
{
    if pkt.len() != 188 || pkt[0] != 0x47 {
        return false;
    }
    if (pkt[1] & 0x80) != 0 {
        psi_buf.clear();
        return false;
    }
    let pusi = (pkt[1] & 0x40) != 0;
    let adapt = (pkt[3] >> 4) & 0x03;
    let payload_offset = match adapt {
        1 => 4,
        3 => {
            let adapt_len = pkt[4] as usize;
            5 + adapt_len
        }
        _ => return false,
    };
    if payload_offset >= 188 {
        return false;
    }
    let payload = &pkt[payload_offset..188];
    let mut parsed_any = false;

    if pusi {
        if payload.is_empty() {
            return false;
        }
        let pointer = payload[0] as usize;
        if 1 + pointer > payload.len() {
            psi_buf.clear();
            return false;
        }
        if !psi_buf.is_empty() && pointer > 0 {
            psi_buf.extend_from_slice(&payload[1..1 + pointer]);
            if process_psi_buffer(psi_buf, &mut on_section) {
                parsed_any = true;
            }
        }
        psi_buf.clear();
        psi_buf.extend_from_slice(&payload[1 + pointer..]);
        if process_psi_buffer(psi_buf, &mut on_section) {
            parsed_any = true;
        }
    } else if !psi_buf.is_empty() {
        psi_buf.extend_from_slice(payload);
        if process_psi_buffer(psi_buf, &mut on_section) {
            parsed_any = true;
        }
        if psi_buf.len() > 8192 {
            psi_buf.clear();
        }
    }
    parsed_any
}

fn process_psi_buffer<F>(psi_buf: &mut Vec<u8>, on_section: &mut F) -> bool
where
    F: FnMut(&[u8]),
{
    let mut parsed = false;
    loop {
        if psi_buf.is_empty() {
            break;
        }
        if psi_buf[0] == 0xFF {
            psi_buf.clear();
            break;
        }
        if psi_buf.len() < 3 {
            break;
        }
        let section_len = (((psi_buf[1] & 0x0F) as usize) << 8) | (psi_buf[2] as usize);
        if section_len > 4093 {
            psi_buf.clear();
            break;
        }
        let total_len = 3 + section_len;
        if psi_buf.len() < total_len {
            break;
        }
        on_section(&psi_buf[..total_len]);
        parsed = true;
        psi_buf.drain(..total_len);
    }
    parsed
}

fn parse_nit_section(section: &[u8], entries: &mut BTreeMap<(u16, u16), CnitTsEntry>) {
    if section.len() < 16 {
        return;
    }
    let table_id = section[0];
    if table_id != 0x40 && table_id != 0x41 {
        return;
    }
    let is_actual = table_id == 0x40;
    let nd_len = (((section[8] & 0x0F) as usize) << 8) | (section[9] as usize);
    let mut idx = 10 + nd_len;
    let end = section.len() - 4;
    if idx + 2 > end {
        return;
    }
    let ts_loop_len = (((section[idx] & 0x0F) as usize) << 8) | (section[idx + 1] as usize);
    idx += 2;
    let ts_end = (idx + ts_loop_len).min(end);

    while idx + 6 <= ts_end {
        let tsid = ((section[idx] as u16) << 8) | (section[idx + 1] as u16);
        let onid = ((section[idx + 2] as u16) << 8) | (section[idx + 3] as u16);
        let td_len = (((section[idx + 4] & 0x0F) as usize) << 8) | (section[idx + 5] as usize);
        idx += 6;
        if idx + td_len > ts_end {
            break;
        }
        let mut freq_key = 0u32;
        let mut frame_type = 0x0Fu8;
        let mut sids: Vec<(u16, u8)> = Vec::new();

        let mut pos = idx;
        let desc_end = idx + td_len;
        while pos + 2 <= desc_end {
            let dtag = section[pos];
            let dlen = section[pos + 1] as usize;
            pos += 2;
            if pos + dlen > desc_end {
                break;
            }
            let dbody = &section[pos..pos + dlen];
            if dtag == 0x44 && dlen >= 11 {
                let b0 = dbody[0];
                let b1 = dbody[1];
                let b2 = dbody[2];
                let mhz = (b0 >> 4) as u32 * 1000
                    + (b0 & 0x0F) as u32 * 100
                    + (b1 >> 4) as u32 * 10
                    + (b1 & 0x0F) as u32;
                let frac = (b2 >> 4) as u32;
                freq_key = mhz * 10 + frac;
                frame_type = (dbody[5] >> 4) & 0x0F;
            } else if dtag == 0x41 {
                let mut k = 0;
                while k + 3 <= dlen {
                    let sid = ((dbody[k] as u16) << 8) | (dbody[k + 1] as u16);
                    let stype = dbody[k + 2];
                    sids.push((sid, stype));
                    k += 3;
                }
            }
            pos += dlen;
        }

        // 自ネットワークのC-NIT情報を優先して保持する
        let should_update = match entries.get(&(onid, tsid)) {
            Some(existing) => is_actual || !existing.is_actual,
            None => true,
        };
        if should_update && freq_key > 0 {
            entries.insert(
                (onid, tsid),
                CnitTsEntry {
                    freq_key,
                    frame_type,
                    services: sids,
                    is_actual,
                },
            );
        }

        idx += td_len;
    }
}

fn parse_sdt_section(
    section: &[u8],
    services: &mut HashMap<(u16, u16, u16), (String, u8, bool)>,
) -> bool {
    if section.len() < 15 {
        return false;
    }
    let table_id = section[0];
    if table_id != 0x42 && table_id != 0x46 {
        return false;
    }
    let is_actual = table_id == 0x42;
    let tsid = ((section[3] as u16) << 8) | (section[4] as u16);
    let onid = ((section[8] as u16) << 8) | (section[9] as u16);

    let end = section.len() - 4;
    let mut idx = 11;

    while idx + 5 <= end {
        let sid = ((section[idx] as u16) << 8) | (section[idx + 1] as u16);
        let desc_loop_len =
            (((section[idx + 3] & 0x0F) as usize) << 8) | (section[idx + 4] as usize);
        idx += 5;

        if idx + desc_loop_len > end {
            break;
        }

        let mut pos = idx;
        let desc_end = idx + desc_loop_len;
        idx = desc_end;

        while pos + 2 <= desc_end {
            let dtag = section[pos];
            let dlen = section[pos + 1] as usize;
            pos += 2;
            if pos + dlen > desc_end {
                break;
            }
            if dtag == 0x48 && dlen >= 3 {
                let dbody = &section[pos..pos + dlen];
                let stype = dbody[0];
                let prov_len = dbody[1] as usize;
                if 2 + prov_len < dlen {
                    let name_len = dbody[2 + prov_len] as usize;
                    let name_start = 3 + prov_len;
                    if name_start + name_len <= dlen {
                        let raw_name = &dbody[name_start..name_start + name_len];
                        let sname = decode_arib_string(raw_name);
                        if !sname.is_empty() && sid > 0 {
                            let should_insert = match services.get(&(onid, tsid, sid)) {
                                Some((_, _, prev_actual)) => is_actual || !*prev_actual,
                                None => true,
                            };
                            if should_insert {
                                services.insert((onid, tsid, sid), (sname, stype, is_actual));
                            }
                        }
                    }
                }
            }
            pos += dlen;
        }
    }
    is_actual
}

fn decode_arib_string(raw: &[u8]) -> String {
    let mut gl_set: u8 = 0;
    let mut gr_set: u8 = 2;
    let mut single_shift: Option<u8> = None;

    let mut out = String::new();
    let mut i = 0;
    let n = raw.len();

    while i < n {
        let b = raw[i];

        // エスケープシーケンスを処理する
        if b == 0x1B {
            i += 1;
            if i >= n {
                break;
            }
            let b2 = raw[i];
            match b2 {
                0x6E => {
                    gl_set = 2;
                    i += 1;
                    continue;
                }
                0x6F => {
                    gl_set = 3;
                    i += 1;
                    continue;
                }
                0x7E => {
                    gr_set = 1;
                    i += 1;
                    continue;
                }
                0x7D => {
                    gr_set = 2;
                    i += 1;
                    continue;
                }
                0x7C => {
                    gr_set = 3;
                    i += 1;
                    continue;
                }
                0x24 => {
                    i += 1;
                    if i < n && (raw[i] == 0x28 || raw[i] == 0x29 || raw[i] == 0x2A || raw[i] == 0x2B) {
                        i += 1;
                    }
                    if i < n {
                        i += 1;
                    }
                    continue;
                }
                0x28..=0x2B => {
                    i += 2;
                    continue;
                }
                _ => {
                    i += 1;
                    continue;
                }
            }
        }

        // ロッキングシフトを処理する
        if b == 0x0E {
            gl_set = 1;
            i += 1;
            continue;
        } else if b == 0x0F {
            gl_set = 0;
            i += 1;
            continue;
        }

        // シングルシフトを処理する
        if b == 0x19 {
            single_shift = Some(2);
            i += 1;
            continue;
        } else if b == 0x1D {
            single_shift = Some(3);
            i += 1;
            continue;
        }

        // 制御コードをスキップする
        if b <= 0x20 || b == 0x7F || (0x80..=0xA0).contains(&b) || b == 0xFF {
            if b == 0x20 || b == 0xA0 {
                out.push(' ');
            }
            i += 1;
            continue;
        }

        let is_gr = (b & 0x80) != 0;
        let active_set = if let Some(ss) = single_shift.take() {
            ss
        } else if is_gr {
            gr_set
        } else {
            gl_set
        };

        let c1 = b & 0x7F;
        match active_set {
            // 1バイト英数字およびプロポーショナル英数字
            1 | 2 => {
                out.push(c1 as char);
                i += 1;
            }
            // 1バイト平仮名および片仮名
            3 => {
                if let Some(ch) = decode_jis_2byte(0x25, c1) {
                    out.push_str(&ch);
                }
                i += 1;
            }
            // 2バイトJIS漢字および追加記号
            _ => {
                if i + 1 < n {
                    let c2 = raw[i + 1] & 0x7F;
                    if (0x21..=0x7E).contains(&c1) && (0x21..=0x7E).contains(&c2) {
                        if let Some(ch) = decode_jis_2byte(c1, c2) {
                            out.push_str(&ch);
                        }
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }

    // 全角英数字を半角に正規化する
    let mut normalized = String::with_capacity(out.len());
    for ch in out.chars() {
        let code = ch as u32;
        if (0xFF01..=0xFF5E).contains(&code) {
            if let Some(ascii_ch) = char::from_u32(code - 0xFEE0) {
                normalized.push(ascii_ch);
                continue;
            }
        } else if ch == '\u{3000}' {
            normalized.push(' ');
            continue;
        }
        normalized.push(ch);
    }

    // 連続する空白を整理する
    let mut cleaned = String::new();
    let mut prev_is_space = false;
    for ch in normalized.trim().chars() {
        if ch.is_whitespace() {
            if !prev_is_space {
                cleaned.push(' ');
                prev_is_space = true;
            }
        } else {
            cleaned.push(ch);
            prev_is_space = false;
        }
    }
    cleaned
}

fn decode_jis_2byte(c1: u8, c2: u8) -> Option<String> {
    let row = c1.wrapping_sub(0x20);
    let cell = c2.wrapping_sub(0x20);

    // ARIB STD-B24の追加記号領域をデコードする
    if row >= 85 {
        let sym = match (row, cell) {
            (85, 1..=26) => char::from_u32('0' as u32 + cell as u32)?.to_string(),
            (86, 1) => "HV".to_string(),
            (86, 2) => "SD".to_string(),
            (86, 3) => "P".to_string(),
            (86, 4) => "W".to_string(),
            (86, 5) => "MV".to_string(),
            (86, 8) => "双".to_string(),
            (86, 9) => "デ".to_string(),
            (86, 10) => "S".to_string(),
            (86, 11) => "二".to_string(),
            (86, 12) => "多".to_string(),
            (86, 13) => "解".to_string(),
            (86, 14) => "SS".to_string(),
            (86, 15) => "B".to_string(),
            (86, 16) => "N".to_string(),
            (86, 21) => "無料".to_string(),
            (86, 22) => "有料".to_string(),
            (86, 23) => "PPV".to_string(),
            (86, 26) => "前".to_string(),
            (86, 27) => "後".to_string(),
            (86, 28) => "再".to_string(),
            (86, 29) => "新".to_string(),
            (86, 30) => "初".to_string(),
            (86, 31) => "終".to_string(),
            (86, 32) => "生".to_string(),
            (86, 35) => "声".to_string(),
            (86, 36) => "吹".to_string(),
            (86, 37) => "字".to_string(),
            (86, 38) => "秘".to_string(),
            (90, 48) => "ほか".to_string(),
            _ => return None,
        };
        return Some(sym);
    }

    let euc = [c1 | 0x80, c2 | 0x80];
    let (cow, _, had_errors) = encoding_rs::EUC_JP.decode(&euc);
    if !had_errors {
        Some(cow.into_owned())
    } else {
        None
    }
}

fn generate_channels_yaml(services: &[ScannedChannel]) -> String {
    let mut out = String::new();
    for s in services {
        let safe_name = s.name.replace('\\', "\\\\").replace('"', "\\\"");
        out.push_str(&format!("- name: \"{}\"\n", safe_name));
        out.push_str(&format!("  type: {}\n", s.channel_type));
        out.push_str(&format!("  channel: '{}'\n", s.channel_str));
        if let Some(sid) = s.service_id {
            out.push_str(&format!("  serviceId: {}\n", sid));
        }

        // 放送種別に応じて有効無効を設定する
        let is_disabled = match s.service_type {
            Some(0x01) | Some(0x02) | Some(0xA1) | Some(0xA2) | Some(0xAD) => false,
            Some(_) => true,
            None => false,
        };
        out.push_str(&format!("  isDisabled: {}\n", is_disabled));
    }
    out
}
