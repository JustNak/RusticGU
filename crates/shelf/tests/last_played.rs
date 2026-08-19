//! Last-played is Steam `LastPlayed` + itch `localLastRunAt` only.
//! ACF `LastUpdated` is ignored. Other stores stay `None`.

use std::time::{Duration, SystemTime};

use shelf::{
    last_played_from_acf, last_played_from_itch_local_last_run_at, last_played_from_steam_localconfig,
    recommend, safe_last_played_source, CompactPolicy, PolicyInput, ShelfConfig, default_denylist,
};

#[test]
fn steam_last_played_key_only() {
    let vdf = r#"
"UserLocalConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"apps"
				{
					"570"
					{
						"LastPlayed"		"1700000100"
						"Playtime"		"99"
					}
				}
			}
		}
	}
}
"#;
    let t = last_played_from_steam_localconfig(vdf, 570).expect("LastPlayed");
    assert_eq!(
        t,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_100)
    );
    assert!(last_played_from_steam_localconfig(vdf, 440).is_none());
}

#[test]
fn itch_local_last_run_at_only() {
    let json = r#"{
      "id": "cave-1",
      "stats": { "localLastRunAt": 1700000200 },
      "game": { "title": "Celeste" }
    }"#;
    let t = last_played_from_itch_local_last_run_at(json).expect("localLastRunAt");
    assert_eq!(
        t,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_200)
    );
}

#[test]
fn acf_last_updated_is_not_last_played() {
    let acf = r#"
"AppState"
{
	"appid"		"570"
	"LastUpdated"		"1700000300"
	"BytesDownloaded"		"1"
}
"#;
    assert_eq!(last_played_from_acf(acf), None);
    assert_eq!(last_played_from_steam_localconfig(acf, 570), None);
}

#[test]
fn other_stores_have_no_safe_source() {
    for store in ["epic", "gog", "xbox", "battlenet", "ea", "ubisoft", "riot"] {
        assert_eq!(safe_last_played_source(store), None);
    }
}

#[test]
fn unknown_last_played_is_conservative_lzx_not_a_fabricated_timestamp() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let mut input = PolicyInput::new("Hades");
    input.last_played = None;
    input.store_id = Some("epic");
    let policy = recommend(&input, now, &ShelfConfig::default(), &default_denylist());
    assert_eq!(policy, CompactPolicy::Lzx);
}
