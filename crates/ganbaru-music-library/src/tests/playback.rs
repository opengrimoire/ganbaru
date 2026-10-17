use super::super::playback;
use super::*;
use super::{playlist, pool, seed_item};

fn update(
    playlist_id: Option<&str>,
    outcome: MusicListeningOutcome,
    occurred_at: i64,
) -> MusicListeningUpdate {
    MusicListeningUpdate {
        playlist_id: playlist_id.map(str::to_string),
        item_id: "item-1".to_string(),
        selection_kind: MusicSelectionKind::Automatic,
        outcome,
        occurred_at,
    }
}

#[test]
fn listening_updates_keep_aggregates_and_recent_selections_separate() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed_item(&pool, "item-1", "local:item-1").await;
        writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();

        playback::record_listening(
            &pool,
            update(
                Some("playlist-1"),
                MusicListeningOutcome::Started,
                1_700_000_000_001,
            ),
        )
        .await
        .unwrap();
        playback::record_listening(
            &pool,
            update(
                Some("playlist-1"),
                MusicListeningOutcome::Completed,
                1_700_000_000_002,
            ),
        )
        .await
        .unwrap();
        playback::record_listening(
            &pool,
            update(
                Some("playlist-1"),
                MusicListeningOutcome::Skipped,
                1_700_000_000_003,
            ),
        )
        .await
        .unwrap();

        let statistics = sqlx::query_as::<_, (Option<i64>, i64, i64, i64)>(
            "SELECT last_played_at_ms, play_count, completion_count, skip_count
             FROM music_listening_statistics WHERE item_id = 'item-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            statistics,
            (Some(1_700_000_000_001), 1, 1, 1),
            "completion and skip updates must not move last played time"
        );
        let recent = playback::recent_selections(&pool, Some("playlist-1".to_string()), 10)
            .await
            .unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].selected_at_ms, 1_700_000_000_001);
    });
}

#[test]
fn recent_selections_are_newest_first_and_bounded_per_playlist() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed_item(&pool, "item-1", "local:item-1").await;
        writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();

        for index in 0..70 {
            let mut request = update(
                Some("playlist-1"),
                MusicListeningOutcome::Started,
                1_700_000_000_001 + index,
            );
            request.selection_kind = if index == 69 {
                MusicSelectionKind::Manual
            } else {
                MusicSelectionKind::Automatic
            };
            playback::record_listening(&pool, request).await.unwrap();
        }

        let retained: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_recent_selections WHERE playlist_id = 'playlist-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(retained, 64);
        let recent = playback::recent_selections(&pool, Some("playlist-1".to_string()), 64)
            .await
            .unwrap();
        assert_eq!(recent.first().unwrap().selected_at_ms, 1_700_000_000_070);
        assert_eq!(recent.last().unwrap().selected_at_ms, 1_700_000_000_007);
        assert!(
            playback::recent_selections(&pool, Some("playlist-1".to_string()), 65,)
                .await
                .is_err()
        );
    });
}
