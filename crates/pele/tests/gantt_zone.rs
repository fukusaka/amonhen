//! gantt を地方時の時間帯で描く。`data/gantt-berlin-*` は pele 1.13.0 を node で `TZ=Europe/Berlin`、`now` を
//! 2024-10-28 12:00 UTC にして描いたもの。ここでは 2024 年の Europe/Berlin を手で作った時間帯で同じに描けるか
//! 確かめる（システムの時間帯のファイルには頼らない）。

use std::fs;
use std::path::Path;
use std::sync::Arc;

use amonhen_pele::{RenderOptions, TimeZone};

/// 2024 年の Europe/Berlin: 3 月 31 日 01:00 UTC から 10 月 27 日 01:00 UTC まで +2 時間、ほかは +1 時間。
struct Berlin2024;

const SPRING: f64 = 1_711_846_800_000.0;
const AUTUMN: f64 = 1_729_990_800_000.0;

impl TimeZone for Berlin2024 {
    fn offset_ms(&self, utc_ms: f64) -> f64 {
        if (SPRING..AUTUMN).contains(&utc_ms) {
            7_200_000.0
        } else {
            3_600_000.0
        }
    }
}

fn options(zone: Option<Arc<dyn TimeZone>>) -> RenderOptions {
    RenderOptions {
        now: Some(1_730_116_800_000.0),
        time_zone: zone,
        ..Default::default()
    }
}

#[test]
fn draws_like_pele_in_a_zone_with_summer_time() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    for name in [
        "gantt-berlin-spring",
        "gantt-berlin-autumn",
        "gantt-berlin-days",
    ] {
        let input = fs::read_to_string(dir.join(format!("{name}.mmd"))).unwrap();
        let want = fs::read_to_string(dir.join(format!("{name}.svg"))).unwrap();
        let zoned =
            amonhen_pele::render_with(&input, &options(Some(Arc::new(Berlin2024)))).unwrap();
        assert_eq!(zoned.svg, want, "{name}");
        // 時間帯がなければ UTC で数え、違う図になる。
        let utc = amonhen_pele::render_with(&input, &options(None)).unwrap();
        assert_ne!(utc.svg, want, "{name}");
    }
}

#[test]
fn reads_skipped_and_repeated_local_times_as_v8_does() {
    use amonhen_pele::diagrams::gantt::js_date as d;
    let zone: Arc<dyn TimeZone> = Arc::new(Berlin2024);
    d::with_time_zone(Some(zone), || {
        // 3 月 31 日 02:30 はない: 変わる前のずれ（+1）で読み、03:30 になる。
        let skipped = d::local(2024.0, 2.0, 31.0, 2.0, 30.0, 0.0, 0.0);
        assert_eq!(skipped, SPRING + 1_800_000.0);
        assert_eq!(d::get_hours(skipped), 3.0);
        // 10 月 27 日 02:30 は 2 度ある: 早い方（+2）。
        let repeated = d::local(2024.0, 9.0, 27.0, 2.0, 30.0, 0.0, 0.0);
        assert_eq!(repeated, AUTUMN - 1_800_000.0);
        assert_eq!(d::get_timezone_offset(repeated), -120.0);
        assert_eq!(d::get_timezone_offset(repeated + 3_600_000.0), -60.0);
        assert_eq!(d::get_hours(repeated + 3_600_000.0), 2.0);
        // 日付だけの ISO の形は UTC、時刻つきは地方時。
        assert_eq!(d::parse("2024-07-01"), 1_719_792_000_000.0);
        assert_eq!(d::parse("2024-07-01T00:00"), 1_719_784_800_000.0);
    });
    // 外では UTC に戻る。
    assert_eq!(d::get_timezone_offset(SPRING), 0.0);
}
