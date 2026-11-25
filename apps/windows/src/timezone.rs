//! gantt の日付を数える時間帯（macOS 版の `Settings.ganttTimeZone`、GTK4 版の `theme::gantt_zone`）。名前は IANA の
//! もの（`Asia/Tokyo` など。macOS 版・GTK4 版と同じ設定の値）で、時差は Windows に入っている ICU で求める。空なら
//! OS の時間帯。

use std::sync::Arc;

use windows::Win32::Globalization::{
    U_ZERO_ERROR, UCAL_DEFAULT, UCAL_DST_OFFSET, UCAL_ZONE_OFFSET, UErrorCode, ucal_close,
    ucal_get, ucal_getDefaultTimeZone, ucal_open, ucal_openTimeZones, ucal_setMillis, uenum_close,
    uenum_unext,
};

/// 設定の鍵（macOS 版・GTK4 版と同じ名前）。
pub const GANTT_TIME_ZONE: &str = "ganttTimeZone";

/// 選べる時間帯の地域（IANA の名前の最初の部分。macOS 版のメニューの下の階層）。
const REGIONS: &[&str] = &[
    "Africa",
    "America",
    "Antarctica",
    "Arctic",
    "Asia",
    "Atlantic",
    "Australia",
    "Europe",
    "Indian",
    "Pacific",
];

/// IANA の名前の時間帯（ICU で時差を求める）。
struct IcuZone(Vec<u16>);

impl amonhen_core::diagram::TimeZone for IcuZone {
    fn offset_ms(&self, utc_ms: f64) -> f64 {
        if !utc_ms.is_finite() {
            return 0.0;
        }
        offset_ms(&self.0, utc_ms).unwrap_or(0.0)
    }
}

/// 時間帯 `zone`（UTF-16 の名前）の、`utc_ms` での 地方時 − UTC（ミリ秒）。
fn offset_ms(zone: &[u16], utc_ms: f64) -> Option<f64> {
    let mut status: UErrorCode = U_ZERO_ERROR;
    unsafe {
        let calendar = ucal_open(
            zone.as_ptr(),
            zone.len() as i32,
            windows::core::PCSTR::null(),
            UCAL_DEFAULT,
            &mut status,
        );
        if calendar.is_null() || status.0 > 0 {
            return None;
        }
        ucal_setMillis(calendar, utc_ms, &mut status);
        let raw = ucal_get(calendar.cast_const().cast(), UCAL_ZONE_OFFSET, &mut status);
        let dst = ucal_get(calendar.cast_const().cast(), UCAL_DST_OFFSET, &mut status);
        ucal_close(calendar);
        (status.0 <= 0).then(|| f64::from(raw) + f64::from(dst))
    }
}

/// OS の時間帯の IANA の名前（ICU の既定）。
fn system_zone() -> Vec<u16> {
    let mut buffer = [0u16; 128];
    let mut status: UErrorCode = U_ZERO_ERROR;
    let len =
        unsafe { ucal_getDefaultTimeZone(buffer.as_mut_ptr(), buffer.len() as i32, &mut status) };
    if status.0 > 0 || len <= 0 {
        return "UTC".encode_utf16().collect();
    }
    buffer[..len as usize].to_vec()
}

/// 今の gantt の時間帯（設定の名前、空なら OS の時間帯）。
pub fn gantt_zone() -> Arc<dyn amonhen_core::diagram::TimeZone> {
    let name = crate::prefs::string(GANTT_TIME_ZONE).unwrap_or_default();
    let zone = if name.is_empty() {
        system_zone()
    } else {
        name.encode_utf16().collect()
    };
    Arc::new(IcuZone(zone))
}

/// 選べる時間帯の IANA の名前を、地域ごとに（地域の順、名前の順）。
pub fn zone_regions() -> Vec<(String, Vec<String>)> {
    let mut names = Vec::new();
    let mut status: UErrorCode = U_ZERO_ERROR;
    unsafe {
        let list = ucal_openTimeZones(&mut status);
        if !list.is_null() && status.0 <= 0 {
            loop {
                let mut len = 0;
                let item = uenum_unext(list, &mut len, &mut status);
                if item.is_null() || status.0 > 0 {
                    break;
                }
                names.push(String::from_utf16_lossy(std::slice::from_raw_parts(
                    item,
                    len as usize,
                )));
            }
            uenum_close(list);
        }
    }
    group(names)
}

/// 名前を地域ごとに分ける（地域の名前を持つもの、`Etc/` などは除く）。
fn group(names: Vec<String>) -> Vec<(String, Vec<String>)> {
    REGIONS
        .iter()
        .filter_map(|region| {
            let prefix = format!("{region}/");
            let mut list: Vec<String> = names
                .iter()
                .filter(|n| n.starts_with(&prefix))
                .cloned()
                .collect();
            list.sort();
            list.dedup();
            (!list.is_empty()).then(|| ((*region).to_owned(), list))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_names_by_region() {
        let names = [
            "Asia/Tokyo",
            "Etc/GMT+1",
            "EST",
            "America/New_York",
            "Asia/Seoul",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(
            group(names),
            [
                ("America".to_owned(), vec!["America/New_York".to_owned()]),
                (
                    "Asia".to_owned(),
                    vec!["Asia/Seoul".to_owned(), "Asia/Tokyo".to_owned()]
                ),
            ]
        );
    }

    #[test]
    fn icu_knows_tokyo_and_new_york_summer_time() {
        let tokyo: Vec<u16> = "Asia/Tokyo".encode_utf16().collect();
        assert_eq!(offset_ms(&tokyo, 0.0), Some(9.0 * 3_600_000.0));
        // 2026-07-01 の New York は夏時間（UTC−4）。
        let new_york: Vec<u16> = "America/New_York".encode_utf16().collect();
        assert_eq!(
            offset_ms(&new_york, 1_782_864_000_000.0),
            Some(-4.0 * 3_600_000.0)
        );
    }
}
