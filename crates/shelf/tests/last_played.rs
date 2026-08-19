//! Last-played is Steam `LastPlayed` + itch `localLastRunAt` only.
//! ACF `LastUpdated` is ignored. Other stores stay `None`.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use shelf::{
    last_played_from_acf, last_played_from_itch_local_last_run_at, last_played_from_steam_localconfig,
    last_played_unix_from_steam_userdata, recommend, safe_last_played_source, CompactPolicy,
    PolicyInput, ShelfConfig, default_denylist,
};

fn steam_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/steam")
}

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
fn steam_userdata_path_reads_localconfig_last_played_only() {
    let userdata = steam_fixture_root().join("userdata");
    let user123 = userdata.join("123");
    assert_eq!(
        last_played_unix_from_steam_userdata(&user123, 570),
        Some(1_700_000_100),
        "userdata\\{{id}}\\config\\localconfig.vdf apps/570/LastPlayed"
    );
    assert_eq!(
        last_played_unix_from_steam_userdata(&userdata, 570),
        Some(1_700_000_100),
        "most recent LastPlayed across user ids 123 (1700000100) and 456 (1690000000)"
    );
    assert_eq!(
        last_played_unix_from_steam_userdata(steam_fixture_root(), 570),
        Some(1_700_000_100),
        "…\\steam root also resolves userdata\\"
    );
    assert_eq!(
        last_played_unix_from_steam_userdata(&userdata, 440),
        None,
        "different appid has no LastPlayed"
    );

    let acf = std::fs::read_to_string(
        steam_fixture_root().join("steamapps/appmanifest_570.acf"),
    )
    .unwrap();
    assert!(acf.contains("LastUpdated"));
    assert_eq!(last_played_from_acf(&acf), None);
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
