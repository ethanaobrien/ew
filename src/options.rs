use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(short, long, default_value_t = 8080, help = "Port to listen on")]
    pub port: u16,

    #[arg(long, default_value = "./", help = "Path to store database files")]
    pub path: String,

    #[arg(long, default_value_t = false, help = "Serve gree headers with https. WILL NOT ACCEPT HTTPS REQUESTS")]
    pub https: bool,

    #[arg(long, default_value = "http://127.0.0.1:51376", help = "Address to NPPS4 server for sif account linking")]
    pub npps4: String,

    //below options are for the "Help" page

    #[arg(long, default_value = "", help = "Link to patched android global apk for this server.")]
    pub global_android: String,

    #[arg(long, default_value = "", help = "Link to patched android japan apk for this server.")]
    pub japan_android: String,

    #[arg(long, default_value = "", help = "Link to patched iOS global apk for this server.")]
    pub global_ios: String,

    #[arg(long, default_value = "", help = "Link to patched iOS japan apk for this server.")]
    pub japan_ios: String,

    #[arg(long, default_value = "", help = "Link to asset server.")]
    pub assets_url: String,

    #[arg(long, default_value_t = 0, help = "Max time returned by the server, in the JSON \"timestamp\" key.")]
    pub max_time: u64,

    #[arg(long, default_value_t = false, help = "Disable webui, act completely like the original server")]
    pub hidden: bool,

    #[arg(long, default_value_t = false, help = "Enable the custom songs feature (upload/browse/download). Disabled by default; every custom-songs endpoint and webui element is hidden unless this is set")]
    pub enable_custom_songs: bool,

    #[arg(long, default_value_t = false, help = "Purge dead user accounts on startup")]
    pub purge: bool,

    #[arg(long, default_value_t = false, help = "Disable user account imports")]
    pub disable_imports: bool,

    #[arg(long, default_value_t = false, help = "Disable user account exports")]
    pub disable_exports: bool,

    #[arg(long, default_value = "", help = "Asset hash for English iOS client.")]
    pub en_ios_asset_hash: String,

    #[arg(long, default_value = "", help = "Asset hash for JP iOS client.")]
    pub jp_ios_asset_hash: String,

    #[arg(long, default_value = "", help = "Asset hash for English Android client.")]
    pub en_android_asset_hash: String,

    #[arg(long, default_value = "", help = "Asset hash for JP Android client.")]
    pub jp_android_asset_hash: String,

    #[arg(long, default_value = "", help = "Asset version for windows client.")]
    pub windows_asset_version: String,

    #[arg(long, default_value = "", help = "Asset hash for windows client.")]
    pub windows_asset_hash: String,

    #[arg(long, default_value = "", help = "Path to image assets.")]
    pub image_asset_path: String,

    #[arg(long, default_value = "", help = "Optional directory to load asset lists and master data CSVs from at runtime. Layout mirrors the bundled assets (asset_lists/, csv/, csv-en/). Missing files fall back to the internal copies.")]
    pub masterdata: String,

    #[arg(long = "mod", value_name = "DIR", action = clap::ArgAction::Append, help = "Path to a mod directory layered on top of --masterdata + the bundled defaults. May be passed multiple times. Each mod dir mirrors the masterdata layout (asset_lists/, csv/, csv-en/, userdata/) but only needs to include the files it adds rows to. CSV rows merge by primary key (first column), asset_lists entries merge by m_identifier, new_user.json top-level arrays union. Later --mod wins on collisions.")]
    pub mods: Vec<String>
}

pub fn get_args() -> Args {
    let mut args = Args::parse();
    crate::runtime::overlay_args(&mut args);
    args
}
