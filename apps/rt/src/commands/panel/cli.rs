use clap::Subcommand;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum PanelCmd {
    /// Read or explicitly edit project ideas in the existing pending ledger.
    #[command(display_order=34)]
    Ideas { #[arg(long,default_value=".")] root:PathBuf, #[arg(long)] json:Option<String> },
    /// Read or explicitly edit presentation profiles; never changes execution.
    #[command(display_order=35)]
    PanelPreferences { #[arg(long,default_value=".")] root:PathBuf, #[arg(long)] json:Option<String> },
    /// Preview consequences without executing the supplied operation.
    #[command(display_order=36)]
    Impact { #[arg(long,default_value=".")] root:PathBuf, #[arg(long)] json:String },
    /// Export sanitized diagnostics locally, or record metadata from stdin.
    #[command(display_order=37)]
    PanelDiagnostics { #[arg(long,default_value=".")] root:PathBuf, #[arg(long)] record:bool, #[arg(long,conflicts_with="record")] export:bool },
    /// Read safe project settings, or apply an explicit versioned patch.
    #[command(display_order = 31)]
    Settings {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// {schema_version:1,revision,changes:{vectors,jev,rtk,build_command,test_command,lint_command,max_compiling_waves,...}}; never credentials.
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
        /// Read one receipt of this wave whole, with the command as recorded.
        #[arg(long, requires = "observations", conflicts_with = "observations_before")]
        receipt: Option<String>,
        /// Query only the visible dashboard section.
        #[arg(long,value_parser=clap::builder::PossibleValuesParser::new(super::dashboard::SECTIONS.iter().copied()))]
        section:Option<String>,
        /// Return unchanged without rebuilding a projection when sources match.
        #[arg(long,requires="section")]
        if_revision:Option<String>,
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
        PanelCmd::Ideas {root,json}=>println!("{}",super::safe_value(crate::commands::event::pending::ideas::run(&root,json.as_deref()))),
        PanelCmd::PanelPreferences {root,json}=>println!("{}",super::safe_value(super::preferences::run(&root,json.as_deref()))),
        PanelCmd::Impact {root,json}=>println!("{}",super::impact::preview(&root,&json)),
        PanelCmd::PanelDiagnostics {root,record,export}=>{
            let root=mustard_core::io::spec_events::spec_root(&root);
            let mut result=if record {read_stdin().map_or_else(||serde_json::json!({"ok":false,"reason":"diagnostic-invalid"}),|value|super::diagnostics::record(&root,&value))}
                else {super::diagnostics::snapshot(&root)};
            if export {
                if !mustard_core::ProjectConfig::exists(&root) {println!("{}",serde_json::json!({"ok":false,"reason":"project-not-initialized"}));return;}
                let path=root.join(".claude/mustard/panel/diagnostics-export.json");
                let safe=super::safe_value(result.clone());
                result=match mustard_core::io::fs::write_atomic(&path,safe.to_string().as_bytes()) {
                    Ok(())=>serde_json::json!({"ok":true,"path":path,"published":false}),
                    Err(_)=>serde_json::json!({"ok":false,"reason":"diagnostics-export-failed"}),
                };
            }
            println!("{}",super::safe_value(result));
        },
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
        PanelCmd::Panel { root, spec, refresh_consumption, session, wave, observations, observations_before,receipt,section,if_revision } => {
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
                // A conta do gasto também fecha o histórico do projeto e o devolve em `project`.
                let mut refresh=crate::commands::spec::spend::spend_at(
                    &crate::commands::spec::spend::SpendOpts{root:root.clone(),publish:false,republish:false,url:None},&machine);
                refresh["waves"]=super::wave_usage::refresh(&root,spec.as_deref(),session.as_deref(),machine.config.as_deref());
                refresh
            });
            let mut result=if let Some(section)=section.as_deref() {super::dashboard::query(&root,spec.as_deref(),session.as_deref(),wave,section,if_revision.as_deref())}
                else {super::snapshot_session(&root,spec.as_deref(),session.as_deref())};
            if section.is_none() && let (Some(wave), Some(name)) = (wave, result["selected_spec"].as_str()) {
                let detail = super::wave_detail(&root, name, wave);
                result["wave_detail"] = detail;
            }
            if let Some(refresh)=refresh {result["consumption_refresh"]=serde_json::json!({"ok":refresh["ok"],"reason":refresh["reason"],"project":refresh["project"]});}
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
