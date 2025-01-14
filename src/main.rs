use anyhow::Result;
use flate2::read::MultiGzDecoder;
use std::fs::File;
use std::path::PathBuf;

use lapce_plugin::{
    psp_types::{
        lsp_types::{request::Initialize, DocumentFilter, DocumentSelector, InitializeParams, Url},
        Request,
    },
    register_plugin, Http, LapcePlugin, VoltEnvironment, PLUGIN_RPC,
};
use serde_json::Value;

#[derive(Default)]
struct State {}

register_plugin!(State);

fn initialize(params: InitializeParams) -> Result<()> {
    let document_selector: DocumentSelector = vec![DocumentFilter {
        language: Some(String::from("java")),
        pattern: Some(String::from("**/*.java")),
        scheme: None,
    }];

    let mut server_args = vec![];
    let mut enable_lombok_agent = false;

    if let Some(options) = params.initialization_options.as_ref() {
        if let Some(enable_lombok) = options.get("lombok") {
            enable_lombok_agent = serde_json::from_value(enable_lombok.clone())?;
        }

        if let Some(volt) = options.get("volt") {
            if let Some(args) = volt.get("serverArgs") {
                if let Some(args) = args.as_array() {
                    if !args.is_empty() {
                        server_args = vec![];
                    }
                    for arg in args {
                        if let Some(arg) = arg.as_str() {
                            server_args.push(arg.to_string());
                        }
                    }
                }
            }

            if let Some(server_path) = volt.get("serverPath") {
                if let Some(server_path) = server_path.as_str() {
                    if !server_path.is_empty() {
                        let url = Url::parse(&format!("urn:{}", server_path))?;
                        PLUGIN_RPC.start_lsp(
                            url,
                            server_args,
                            document_selector,
                            params.initialization_options,
                        );
                        return Ok(());
                    }
                }
            }
        }
    }

    let jdtls_file_name = "jdt-language-server-latest";
    let gz_path = PathBuf::from(format!("{jdtls_file_name}.tar.gz"));
    let url = format!(
        "https://download.eclipse.org/jdtls/snapshots/{}.tar.gz",
        jdtls_file_name
    );

    if !PathBuf::from(jdtls_file_name).exists() {
        if !gz_path.exists() {
            let mut resp = Http::get(&url)?;
            let body = resp.body_read_all()?;
            std::fs::write(&gz_path, body)?;
        }

        let tar_gz = std::fs::File::open(gz_path).unwrap();
        let tar = MultiGzDecoder::new(tar_gz);
        let mut archive = tar::Archive::new(tar);
        let dir = PathBuf::from(jdtls_file_name);
        std::fs::create_dir(&dir)?;
        for (_, file) in archive.entries().unwrap().raw(true).enumerate() {
            let mut entry = file?;
            let entry_type = entry.header().entry_type();
            if !entry_type.is_dir() && !entry_type.is_file() {
                continue;
            }

            let entry_path = dir.join(&entry.path()?);
            if entry_type.is_dir() {
                std::fs::create_dir_all(&entry_path)?;
            } else if entry_type.is_file() {
                let mut outfile = File::create(&entry_path)?;
                std::io::copy(&mut entry, &mut outfile)?;
            }
        }
    }

    // Plugin working directory
    let volt_uri = VoltEnvironment::uri()?;
    let base_path = Url::parse(&volt_uri)?;

    if enable_lombok_agent {
        let lombok_jar = "lombok.jar";
        let lombok_url = format!("https://projectlombok.org/downloads/{lombok_jar}");

        if !PathBuf::from(lombok_jar).exists() {
            let mut resp = Http::get(&lombok_url)?;
            let body = resp.body_read_all()?;
            std::fs::write(&lombok_jar, body)?;
        }

        let lombok = base_path.join("lombok.jar")?;
        let lombok = lombok.to_file_path().expect("failed to get file path");
        let lombok = lombok.to_string_lossy();
        server_args.push(format!("--jvm-arg=-javaagent:{lombok}"));
    }

    let jdtls = base_path.join(&format!("{jdtls_file_name}/bin/jdtls"))?;

    PLUGIN_RPC.start_lsp(
        jdtls,
        server_args,
        document_selector,
        params.initialization_options,
    );

    Ok(())
}

impl LapcePlugin for State {
    fn handle_request(&mut self, _id: u64, method: String, params: Value) {
        PLUGIN_RPC.stderr(&format!("{_id}, {method}"));
        #[allow(clippy::single_match)]
        match method.as_str() {
            Initialize::METHOD => {
                let params: InitializeParams = serde_json::from_value(params).unwrap();
                let _ = initialize(params);
            }
            _ => {}
        }
    }
}
