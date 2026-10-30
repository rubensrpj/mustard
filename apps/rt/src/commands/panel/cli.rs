use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum PanelCmd {
    /// Read safe project settings, or apply an explicit versioned patch.
    #[command(display_order = 31)]
    Settings {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// {schema_version:1,revision,changes:{vectors,jev,rtk,build_command,test_command,lint_command,max_compiling_waves,...}}; the publication token is write-only and comes only with --stdin.
        #[arg(long, conflicts_with = "stdin")]
        json: Option<String>,
        /// Read the same patch from standard input, the only way the
        /// publication token is taken: an argument is visible to other processes.
        #[arg(long)]
        stdin: bool,
        /// The whole answer: besides the current values and the revision,
        /// the state of Jev, rtk, the index and the integrations.
        #[arg(long)]
        full: bool,
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
    /// Read one panel screen: the overview, the spec, a page of items, one item, the usage or a page of waves.
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
        /// Read one receipt of this wave whole, with the command as recorded.
        #[arg(long, requires = "observations", conflicts_with = "observations_before")]
        receipt: Option<String>,
        /// The screen to read.
        #[arg(long,value_parser=clap::builder::PossibleValuesParser::new(super::dashboard::SECTIONS.iter().copied()))]
        section:Option<String>,
        /// Return unchanged without rebuilding a projection when sources match.
        #[arg(long,requires="section")]
        if_revision:Option<String>,
        #[command(flatten)]
        screen:Box<ScreenArgs>,
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

/// What the list and item screens of `run panel` were asked for.
#[derive(Debug, Args)]
pub struct ScreenArgs {
    /// The list screen's item kind.
    #[arg(long,requires="section",value_parser=clap::builder::PossibleValuesParser::new(super::sections::KINDS.iter().copied()))]
    kind:Option<String>,
    /// Search the list screen's items by words or file path.
    #[arg(long,requires="section")]
    query:Option<String>,
    /// The list screen's filter.
    #[arg(long,requires="section",value_parser=clap::builder::PossibleValuesParser::new(super::sections::FILTERS.iter().copied()))]
    filter:Option<String>,
    /// Skip this many rows of the list screen.
    #[arg(long,requires="section")]
    offset:Option<usize>,
    /// Rows of the list screen (8 when absent).
    #[arg(long,requires="section")]
    limit:Option<usize>,
    /// The item screen's code.
    #[arg(long,requires="section")]
    item:Option<String>,
}

pub fn dispatch(cmd: PanelCmd) {
    match cmd {
        PanelCmd::Settings {root,json,stdin,full} => {
            let json = crate::shared::json_arg::resolve_opt(json);
            let report=if stdin {
                use std::io::Read;let mut text=String::new();
                match std::io::stdin().take(16_385).read_to_string(&mut text) {
                    Ok(_)=>super::settings::run(&root,Some(&text)),
                    Err(_)=>serde_json::json!({"ok":false,"schema_version":1,"reason":"settings-invalid-request"}),
                }
            } else if let Err(detail) = &json {
                // O arquivo do `--json` que não se lê recusa sem gravar nada.
                serde_json::json!({"ok":false,"schema_version":1,"reason":"settings-invalid-request","detail":detail})
            } else if let Ok(json) = &json && json.as_deref().is_some_and(super::settings::carries_token) {
                serde_json::json!({"ok":false,"schema_version":1,"reason":"settings-token-argument"})
            } else {super::settings::run(&root,json.ok().flatten().as_deref())};
            println!("{}",crate::commands::flow::projection("settings",&report,full).unwrap_or(report));
        }
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
        PanelCmd::Panel { root, spec, refresh_consumption, session, wave, observations, observations_before,receipt,section,if_revision,screen } => {
            if observations {
                let name=spec.or_else(||crate::shared::context::checkout::current_spec(&root.to_string_lossy()));
                let root=mustard_core::io::spec_events::spec_root(&root);
                println!("{}",name.zip(wave).map_or_else(||serde_json::json!({"ok":false,"reason":"observation-needs-spec-and-wave"}),|(name,wave)| {
                    let dispatch=mustard_core::io::spec_events::spec_file(&root,&name).ok()
                        .and_then(|path|mustard_core::io::spec_events::read(&path).ok().flatten())
                        .and_then(|log|log.last_by_wave("send").get(&wave).map(|id|log.dispatch_position(*id)));
                    dispatch.map_or_else(||serde_json::json!({"ok":false,"reason":"observation-no-wave-dispatch"}),|dispatch| {
                        let read=match receipt.as_deref() {
                            Some(id)=>mustard_core::io::observations::detail(&root,&name,dispatch,id),
                            None=>mustard_core::io::observations::page(&root,&name,dispatch,observations_before.as_deref()).map(|mut page| {
                                if page["receipts"].as_array().is_some_and(|rows|!rows.is_empty()) {
                                    page["whole_command"]=serde_json::json!("repeat with --receipt <receipt>");
                                }
                                page
                            }),
                        };
                        match read {
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
                // A conta do gasto também fecha o histórico do projeto e o devolve em `project`,
                // mesmo quando a conta da máquina é recusada.
                let mut refresh=crate::commands::spec::spend::spend_at(
                    &crate::commands::spec::spend::SpendOpts{root:root.clone(),publish:false,republish:false,url:None},&machine);
                refresh["waves"]=super::wave_usage::refresh(&root,spec.as_deref(),session.as_deref(),machine.config.as_deref());
                refresh["copies_disk"]=super::sections::measure_copies_disk(&mustard_core::io::spec_events::spec_root(&root));
                refresh
            });
            let ScreenArgs{kind,query,filter,offset,limit,item}=*screen;
            let params=super::sections::Params{kind,query,filter,offset,limit,item};
            let mut result=if let Some(section)=section.as_deref() {super::dashboard::query_with(&root,spec.as_deref(),session.as_deref(),wave,section,if_revision.as_deref(),&params)}
                else {super::snapshot_session(&root,spec.as_deref(),session.as_deref())};
            if section.is_none() && let (Some(wave), Some(name)) = (wave, result["selected_spec"].as_str()) {
                let detail = super::wave_detail(&root, name, wave);
                result["wave_detail"] = detail;
            }
            if let Some(refresh)=refresh {result["consumption_refresh"]=serde_json::json!({"ok":refresh["ok"],"reason":refresh["reason"],"project":refresh["project"],"copies_disk":refresh["copies_disk"]});}
            println!("{result}");
        },
        PanelCmd::UsageRecord {root,session} => {
            let value=read_stdin();
            println!("{}",value.map_or_else(||serde_json::json!({"ok":false,"reason":"usage-invalid-measurement"}),|value|super::consumption::record(&root,&session,&value)));
        },
    }
}

fn read_stdin()->Option<serde_json::Value> {
    use std::io::Read;let mut text=String::new();
    std::io::stdin().take(65537).read_to_string(&mut text).ok().filter(|_|text.len()<=65536)
        .and_then(|_|serde_json::from_str(&text).ok())
}
