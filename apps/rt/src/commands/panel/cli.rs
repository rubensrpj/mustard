use clap::Subcommand;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum PanelCmd {
    /// Read safe project settings, or apply an explicit versioned patch.
    #[command(display_order = 31)]
    Settings {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// {schema_version:1,revision,changes:{vectors,jev,rtk,...}}; never credentials.
        #[arg(long)]
        json: Option<String>,
    },
    /// Publish an explicit static snapshot using the configured native destination.
    #[command(display_order = 27)]
    Publish {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, conflicts_with_all = ["project", "document"])]
        spec: Option<String>,
        #[arg(long, conflicts_with = "document")]
        project: bool,
        #[arg(long)]
        document: Option<PathBuf>,
        #[arg(long, conflicts_with = "document")]
        include_consumption: bool,
    },
    /// Query local project, specs, execution and measured consumption.
    #[command(display_order = 26)]
    Panel {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        spec: Option<String>,
        /// Refresh the native transcript ledger once; ordinary queries read it.
        #[arg(long)]
        refresh_consumption: bool,
        /// Select the host session's statusline observation.
        #[arg(long)]
        session: Option<String>,
        /// Load the selected wave's plan, exact recorded input and delivery.
        #[arg(long)]
        wave: Option<u64>,
        /// Read only native tool-result receipts for this wave, in bounded pages.
        #[arg(long, requires = "wave", conflicts_with = "refresh_consumption")]
        observations: bool,
        /// Continue a receipt page using its next_before value.
        #[arg(long, requires = "observations")]
        observations_before: Option<String>,
    },
    /// Record a host measurement from standard input, without a model turn.
    #[command(display_order = 29)]
    UsageRecord {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        session: String,
    },
}

pub fn dispatch(cmd: PanelCmd) {
    match cmd {
        PanelCmd::Settings {root,json} => println!("{}",super::settings::run(&root,json.as_deref())),
        PanelCmd::Publish {
            root,
            spec,
            project,
            document,
            include_consumption,
        } => println!(
            "{}",
            if let Some(document) = document {
                super::publication::publish_report(&root, &document)
            } else if project {
                super::publication::publish_project(&root, include_consumption)
            } else {
                let name = spec.or_else(|| {
                    let absolute = std::path::absolute(&root).unwrap_or_else(|_| root.clone());
                    let checkout =
                        mustard_core::io::workspace::anchor_of(&absolute).unwrap_or(absolute);
                    crate::shared::context::checkout::spec_of_checkout_branch(
                        &checkout.to_string_lossy(),
                    )
                });
                name.map_or_else(||serde_json::json!({"ok":false,"reason":"no-current-spec","hint":"Use --project para publicar o projeto sem spec aberta."}),|name|super::publication::publish(&root,&name,include_consumption))
            }
        ),
        PanelCmd::Panel { root, spec, refresh_consumption, session, wave, observations, observations_before } => {
            if observations {
                let name=spec.or_else(||crate::shared::context::checkout::current_spec(&root.to_string_lossy()));
                let root=mustard_core::io::spec_events::spec_root(&root);
                println!("{}",name.zip(wave).map_or_else(||serde_json::json!({"ok":false,"reason":"observation-needs-spec-and-wave"}),|(name,wave)| {
                    let dispatch=mustard_core::io::spec_events::spec_file(&root,&name).ok()
                        .and_then(|path|mustard_core::io::spec_events::read(&path).ok().flatten())
                        .and_then(|log|log.last_by_wave("send").get(&wave).map(|id|log.dispatch_position(*id)));
                    dispatch.map_or_else(||serde_json::json!({"ok":false,"reason":"observation-no-wave-dispatch"}),|dispatch| {
                        match mustard_core::io::observations::page(&root,&name,dispatch,observations_before.as_deref()) {
                            Ok(mut page)=>{page["ok"]=serde_json::json!(true);page["spec"]=serde_json::json!(name);page["wave"]=serde_json::json!(wave);page},
                            Err(reason)=>serde_json::json!({"ok":false,"reason":reason}),
                        }
                    })
                }));
                return;
            }
            let refresh=refresh_consumption.then(||{
                crate::shared::rtk_gain::refresh(&root);
                let machine=crate::commands::spec::spend::Machine::here();
                let mut refresh=crate::commands::spec::spend::spend_at(
                    &crate::commands::spec::spend::SpendOpts{root:root.clone(),publish:false,republish:false,url:None},&machine);
                refresh["project"]=super::consumption::refresh_history(&root,&machine);
                refresh["waves"]=super::wave_usage::refresh(&root,spec.as_deref(),session.as_deref(),machine.config.as_deref());
                refresh
            });
            let mut result=super::snapshot_session(&root,spec.as_deref(),session.as_deref());
            if let (Some(wave), Some(name)) = (wave, result["selected_spec"].as_str()) {
                let detail = super::wave_detail(&root, name, wave);
                result["wave_detail"] = detail;
            }
            if let Some(refresh)=refresh {result["consumption_refresh"]=serde_json::json!({"ok":refresh["ok"],"reason":refresh["reason"],"project":refresh["project"]});}
            println!("{result}");
        },
        PanelCmd::UsageRecord {root,session} => {
            use std::io::Read;
            let mut text=String::new();
            let value=std::io::stdin().take(65537).read_to_string(&mut text).ok().filter(|_|text.len()<=65536).and_then(|_|serde_json::from_str(&text).ok());
            println!("{}",value.map_or_else(||serde_json::json!({"ok":false,"reason":"usage-invalid-measurement"}),|value|super::consumption::record(&root,&session,&value)));
        },
    }
}
