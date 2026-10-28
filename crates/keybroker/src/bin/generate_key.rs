use clap::Parser;
use common::types::MemberId;
use keybroker::{
    DeploymentSecret,
    KeyBroker,
};

#[derive(Parser)]
#[command(
    author,
    version,
    about = "Generate admin and system keys for a Convex instance"
)]
struct Args {
    /// Name of the Convex instance, e.g. `flying-fox-123`
    #[arg(help = "Name of the Convex instance")]
    instance_name: String,

    /// Instance secret (32-byte hex string),
    /// which can be generated with `openssl rand -hex 32`
    #[arg(help = "Instance secret (32-byte hex string)")]
    instance_secret: String,

    /// Generate a system key instead of an admin key.
    /// A system key indicates an operation done "by Convex", like an internal
    /// migration.
    #[arg(long, conflicts_with = "member_id")]
    system_key: bool,

    /// Member ID for the admin key.
    /// An admin key indicates an operation done by an admin, i.e. someone who
    /// can access the instance's dashboard or CLI.
    /// Member ID 0 can be used for generic admin keys, without a specified
    /// member.
    #[arg(long, default_value = "0")]
    member_id: u64,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let deployment_secret = DeploymentSecret::try_from(&args.instance_secret[..])?;
    let broker = KeyBroker::new(&args.instance_name, deployment_secret)?;

    if args.system_key {
        eprintln!("System key:");
        let system_key = broker.issue_system_key();
        println!("{}", system_key.as_str());
    } else {
        if args.member_id == 0 {
            eprintln!("Admin key:");
        } else {
            eprintln!("Admin key for member ID {}:", args.member_id);
        }
        let admin_key = broker.issue_admin_key(MemberId(args.member_id));
        println!("{}", admin_key.as_str());
    }

    Ok(())
}
