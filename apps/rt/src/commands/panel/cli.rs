use clap::Subcommand;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum PanelCmd {
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
        PanelCmd::Panel { root, spec, refresh_consumption, session } => {
            let refresh=refresh_consumption.then(||{
                let machine=crate::commands::spec::spend::Machine::here();
                let mut refresh=crate::commands::spec::spend::spend_at(
                    &crate::commands::spec::spend::SpendOpts{root:root.clone(),publish:false,republish:false,url:None},&machine);
                refresh["project"]=super::consumption::refresh_history(&root,&machine);
                refresh
            });
            let mut result=super::snapshot_session(&root,spec.as_deref(),session.as_deref());
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
