//! Managed asset files that `Local` rows name: the digests the sweep keeps and the manifest
//! checks on the declaration.

use super::block_on;
use super::fixture::{FIXTURE_CLASS, FIXTURE_DOMAIN, fixture_pool, test_engine};
use crate::assets::{AssetClass, LocalAssetColumn, LocalDigest, local_asset_problems};
use crate::manifest::{DeviceTable, DomainManifest};

fn digest(seed: char) -> String {
    std::iter::repeat_n(seed, 64).collect()
}

#[test]
fn local_rows_keep_bare_digests_asset_ids_and_json_references() {
    block_on(async {
        let pool = fixture_pool().await;
        let engine = test_engine();
        let class = FIXTURE_CLASS.name;
        let (bare, id, listed, nested) = (digest('a'), digest('b'), digest('c'), digest('d'));
        let foreign = digest('e');
        sqlx::query(
            "INSERT INTO fixture_asset_pins (id, sha256, asset_id, attrs) VALUES
                 ('pin-1', ?, ?, ?),
                 ('pin-2', 'not-a-digest', ?, '{broken'),
                 ('pin-3', NULL, NULL, ?)",
        )
        .bind(&bare)
        .bind(format!("{class}:{id}"))
        .bind(format!(r#"{{"files": ["{class}:{listed}"]}}"#))
        .bind(format!("other/class:{foreign}"))
        .bind(format!(
            r#"{{"files": {{"cover": "{class}:{nested}"}}, "other": "{class}:{foreign}"}}"#
        ))
        .execute(&pool)
        .await
        .unwrap();
        let mut conn = pool.acquire().await.unwrap();

        let kept = engine.kept_asset_digests(&mut conn).await.unwrap();

        let expected = [bare, id, listed, nested]
            .into_iter()
            .map(|digest| (class, digest))
            .collect();
        assert_eq!(kept, expected);
        assert_eq!(
            engine.local_asset_tables(&FIXTURE_CLASS),
            vec!["fixture_asset_pins"]
        );
    });
}

#[test]
fn removing_the_local_row_drops_its_digest() {
    block_on(async {
        let pool = fixture_pool().await;
        let engine = test_engine();
        sqlx::query("INSERT INTO fixture_asset_pins (id, sha256) VALUES ('pin', ?)")
            .bind(digest('f'))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM fixture_asset_pins")
            .execute(&pool)
            .await
            .unwrap();
        let mut conn = pool.acquire().await.unwrap();

        assert!(
            engine
                .kept_asset_digests(&mut conn)
                .await
                .unwrap()
                .is_empty()
        );
    });
}

const UNDECLARED: &[LocalAssetColumn] = &[LocalAssetColumn {
    column: "sha256",
    json_path: Some("files"),
    digest: LocalDigest::Sha256 {
        class: "fixture/missing",
    },
}];

#[test]
fn malformed_local_asset_columns_are_refused() {
    let local = [DeviceTable {
        name: "fixture_asset_pins",
        triggers: &[],
        assets: UNDECLARED,
    }];
    let leaked: &'static [DeviceTable] = Box::leak(Box::new(local));
    let domain = DomainManifest {
        local: leaked,
        assets: &[],
        ..FIXTURE_DOMAIN
    };

    let problems = local_asset_problems(&domain);

    assert_eq!(
        problems,
        vec![
            "fixture_asset_pins.sha256: names undeclared asset class fixture/missing",
            "fixture_asset_pins.sha256: the domain declares no asset class",
            "fixture_asset_pins.sha256: a JSON path starts at $",
        ]
    );
    let engine_table = DomainManifest {
        local: &[],
        engine: leaked,
        ..FIXTURE_DOMAIN
    };
    assert_eq!(
        local_asset_problems(&engine_table),
        vec!["fixture_asset_pins: only a Local table declares asset columns"]
    );
    assert!(local_asset_problems(&FIXTURE_DOMAIN).is_empty());
}

/// A second class the fixture domain could declare, which its asset id columns do not list.
const OTHER_CLASS: AssetClass = AssetClass {
    name: "fixture/others",
    max_bytes: 1024,
};

#[test]
fn an_asset_id_names_only_the_classes_its_column_lists() {
    let domain = DomainManifest {
        assets: &[FIXTURE_CLASS, OTHER_CLASS],
        ..FIXTURE_DOMAIN
    };
    let listed = LocalDigest::AssetId {
        classes: &[FIXTURE_CLASS.name],
    };
    let sha = digest('a');

    let (class, named) = listed
        .resolve(&domain, &format!("{}:{sha}", FIXTURE_CLASS.name))
        .unwrap();
    assert_eq!((class.name, named), (FIXTURE_CLASS.name, sha.clone()));
    assert!(
        listed
            .resolve(&domain, &format!("{}:{sha}", OTHER_CLASS.name))
            .is_none()
    );
    let bare = LocalDigest::Sha256 {
        class: OTHER_CLASS.name,
    };
    assert!(
        bare.resolve(&domain, &sha)
            .is_some_and(|(class, _)| class.name == OTHER_CLASS.name)
    );
}

const UNLISTED: &[LocalAssetColumn] = &[
    LocalAssetColumn {
        column: "asset_id",
        json_path: None,
        digest: LocalDigest::AssetId { classes: &[] },
    },
    LocalAssetColumn {
        column: "attrs",
        json_path: Some("$.files"),
        digest: LocalDigest::AssetId {
            classes: &["fixture/missing"],
        },
    },
];

#[test]
fn an_asset_id_column_lists_declared_classes() {
    let local = [DeviceTable {
        name: "fixture_asset_pins",
        triggers: &[],
        assets: UNLISTED,
    }];
    let leaked: &'static [DeviceTable] = Box::leak(Box::new(local));
    let domain = DomainManifest {
        local: leaked,
        ..FIXTURE_DOMAIN
    };

    assert_eq!(
        local_asset_problems(&domain),
        vec![
            "fixture_asset_pins.asset_id: names no asset class",
            "fixture_asset_pins.attrs: names undeclared asset class fixture/missing",
        ]
    );
}
