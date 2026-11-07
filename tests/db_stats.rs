use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use netrack::db::{
    Db,
    stats::statistics,
    tests_repo::{self, Metrics, NewSpeedtest, TestKind},
};

fn new_test(metrics: Result<Metrics, String>) -> NewSpeedtest {
    NewSpeedtest {
        kind: TestKind::Auto,
        provider: "cloudflare".into(),
        server_id: None,
        server_name: None,
        server_host: None,
        result_id: None,
        metrics,
        duration: 10.0,
    }
}

fn dl(download: f64) -> Result<Metrics, String> {
    Ok(Metrics {
        ping: 10.0,
        jitter: 1.0,
        download,
        upload: 50.0,
    })
}

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
}

async fn add(db: &Db, m: Result<Metrics, String>, when: DateTime<Utc>) {
    tests_repo::insert(db, new_test(m), when).await.unwrap();
}

#[tokio::test]
async fn summary_avg_min_max_stddev() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    for (i, d) in [100.0, 200.0, 300.0].into_iter().enumerate() {
        add(&db, dl(d), at(2026, 11, 1, 10 + i as u32, 0)).await;
    }
    let s = statistics(&db, day(2026, 11, 1), day(2026, 11, 1))
        .await
        .unwrap();
    assert_eq!(s.total, 3);
    assert_eq!(s.download.avg, Some(200.0));
    assert_eq!(s.download.min, Some(100.0));
    assert_eq!(s.download.max, Some(300.0));
    assert_eq!(s.download.std_dev, Some(81.65));
    assert_eq!(s.download.consistency, Some(59.2));
    // constant metric: zero deviation, full consistency
    assert_eq!(s.ping.std_dev, Some(0.0));
    assert_eq!(s.ping.consistency, Some(100.0));
}

#[tokio::test]
async fn empty_range_has_null_metrics_and_24_buckets() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    let s = statistics(&db, day(2026, 11, 1), day(2026, 11, 3))
        .await
        .unwrap();
    assert_eq!(s.total, 0);
    assert_eq!(s.availability, 100.0);
    assert_eq!(s.download.avg, None);
    assert_eq!(s.download.consistency, None);
    assert_eq!(s.hourly.len(), 24);
    assert!(s.heatmap.is_empty() && s.series.is_empty());
}

#[tokio::test]
async fn failed_rows_excluded_from_metrics_but_counted() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    for i in 0..3 {
        add(&db, dl(100.0), at(2026, 11, 1, 10 + i, 0)).await;
    }
    add(&db, Err("boom".into()), at(2026, 11, 1, 14, 0)).await;
    let s = statistics(&db, day(2026, 11, 1), day(2026, 11, 1))
        .await
        .unwrap();
    assert_eq!(s.total, 4);
    assert_eq!(s.failed, 1);
    assert_eq!(s.availability, 75.0);
    assert_eq!(s.download.avg, Some(100.0));
    assert_eq!(s.series.len(), 3);
    assert_eq!(s.hourly.iter().map(|h| h.count).sum::<u64>(), 3);
}

#[tokio::test]
async fn hourly_has_24_buckets_in_server_tz() {
    let tz: Tz = "America/Sao_Paulo".parse().unwrap();
    let db = Db::open_in_memory(tz).await.unwrap();
    add(&db, dl(100.0), at(2026, 3, 10, 0, 30)).await; // 21:30 on the 9th local
    let s = statistics(&db, day(2026, 3, 9), day(2026, 3, 9))
        .await
        .unwrap();
    assert_eq!(s.hourly.len(), 24);
    for (i, b) in s.hourly.iter().enumerate() {
        assert_eq!(b.hour as usize, i);
    }
    assert_eq!(s.hourly[21].count, 1);
    assert_eq!(s.hourly[21].download, Some(100.0));
    assert_eq!(s.hourly[0].count, 0);
    assert_eq!(s.hourly[0].download, None);
}

#[tokio::test]
async fn heatmap_groups_by_weekday_and_hour() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    // 2026-11-02 is a Monday
    add(&db, dl(100.0), at(2026, 11, 2, 8, 0)).await;
    add(&db, dl(300.0), at(2026, 11, 2, 8, 30)).await;
    add(&db, dl(50.0), at(2026, 11, 3, 9, 0)).await;
    let s = statistics(&db, day(2026, 11, 2), day(2026, 11, 3))
        .await
        .unwrap();
    assert_eq!(s.heatmap.len(), 2);
    let mon = s.heatmap.iter().find(|c| c.weekday == 0).unwrap();
    assert_eq!((mon.hour, mon.download, mon.count), (8, 200.0, 2));
    let tue = s.heatmap.iter().find(|c| c.weekday == 1).unwrap();
    assert_eq!((tue.hour, tue.download, tue.count), (9, 50.0, 1));
}

#[tokio::test]
async fn series_is_downsampled_to_300_points() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    let start = at(2026, 11, 1, 0, 0);
    for i in 0..1000i64 {
        let when = start + Duration::seconds(i * 864); // 10 days
        add(&db, dl(100.0 + (i % 50) as f64), when).await;
    }
    let s = statistics(&db, day(2026, 11, 1), day(2026, 11, 10))
        .await
        .unwrap();
    assert_eq!(s.total, 1000);
    assert!(s.series.len() <= 300, "got {}", s.series.len());
    assert!(s.series.len() > 250);
    let vals: Vec<f64> = s.series.iter().map(|p| p.download.unwrap()).collect();
    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
    let overall = s.download.avg.unwrap();
    assert!(
        (mean - overall).abs() / overall < 0.01,
        "{mean} vs {overall}"
    );
    assert!(s.series.windows(2).all(|w| w[0].t < w[1].t));
}

#[tokio::test]
async fn range_is_inclusive_of_whole_days() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    add(&db, dl(1.0), at(2026, 10, 31, 23, 59)).await;
    add(&db, dl(2.0), at(2026, 11, 1, 0, 0)).await;
    add(&db, dl(3.0), at(2026, 11, 2, 23, 59)).await;
    add(&db, dl(4.0), at(2026, 11, 3, 0, 0)).await;
    let s = statistics(&db, day(2026, 11, 1), day(2026, 11, 2))
        .await
        .unwrap();
    assert_eq!(s.total, 2);
    assert_eq!(s.download.min, Some(2.0));
    assert_eq!(s.download.max, Some(3.0));
}

#[tokio::test]
async fn range_uses_server_tz_midnights() {
    let tz: Tz = "America/Sao_Paulo".parse().unwrap(); // UTC-3 in 2026
    let db = Db::open_in_memory(tz).await.unwrap();
    add(&db, dl(1.0), at(2026, 11, 1, 2, 59)).await; // Oct 31 23:59 local
    add(&db, dl(2.0), at(2026, 11, 1, 3, 0)).await; // Nov 1 00:00 local
    add(&db, dl(3.0), at(2026, 11, 2, 2, 59)).await; // Nov 1 23:59 local
    add(&db, dl(4.0), at(2026, 11, 2, 3, 0)).await; // Nov 2 00:00 local
    let s = statistics(&db, day(2026, 11, 1), day(2026, 11, 1))
        .await
        .unwrap();
    assert_eq!(s.total, 2);
}

#[tokio::test]
async fn nonexistent_midnight_does_not_error() {
    // Sao Paulo skipped 2018-11-04 00:00 (DST start jumped to 01:00)
    let tz: Tz = "America/Sao_Paulo".parse().unwrap();
    let db = Db::open_in_memory(tz).await.unwrap();
    add(&db, dl(1.0), at(2018, 11, 4, 3, 30)).await; // 00:30 -> 01:30 local
    let s = statistics(&db, day(2018, 11, 4), day(2018, 11, 4))
        .await
        .unwrap();
    assert_eq!(s.total, 1);
}

#[tokio::test]
async fn inverted_range_is_an_error() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    assert!(
        statistics(&db, day(2026, 11, 2), day(2026, 11, 1))
            .await
            .is_err()
    );
}

#[tokio::test]
#[ignore = "perf: run with --release -- --ignored"]
async fn stats_on_100k_rows_under_200ms() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    let start = at(2026, 1, 1, 0, 0).timestamp_millis();
    db.call(move |conn| {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO speedtests (created_at, local_hour, local_weekday, kind, provider, \
                 ping, jitter, download, upload, duration, error) \
                 VALUES (?1, ?2, ?3, 'auto', 'cloudflare', ?4, 1.0, ?5, 50.0, 10.0, ?6)",
            )?;
            for i in 0..100_000i64 {
                let created = start + i * 300_000; // every 5 min, ~347 days
                let (h, w) = netrack::db::local_fields(chrono_tz::UTC, created);
                let failed = i % 20 == 0;
                stmt.execute(rusqlite::params![
                    created,
                    h,
                    w,
                    if failed {
                        None
                    } else {
                        Some(10.0 + (i % 7) as f64)
                    },
                    if failed {
                        None
                    } else {
                        Some(100.0 + (i % 400) as f64)
                    },
                    if failed { Some("err") } else { None },
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
    .await
    .unwrap();

    let t = std::time::Instant::now();
    let s = statistics(&db, day(2026, 1, 1), day(2026, 12, 31))
        .await
        .unwrap();
    let elapsed = t.elapsed();
    println!("stats over 100k rows: {elapsed:?}");
    assert_eq!(s.total, 100_000);
    assert!(s.series.len() <= 300);
    assert!(elapsed.as_millis() < 200, "took {elapsed:?}");
}
