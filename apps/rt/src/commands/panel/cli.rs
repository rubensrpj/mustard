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
        PanelCmd::Panel { root, spec } => println!("{}", super::snapshot(&root, spec.as_deref())),
    }
}
