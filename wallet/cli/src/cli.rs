use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use workflow_terminal::Terminal;
// use workflow_terminal::Options;
use crate::actions::*;
use crate::result::Result;
use kaspa_wallet_core::Wallet;
use workflow_log::*;
pub use workflow_terminal::{
    parse,
    Cli,
    Result as TerminalResult,
    Options as TerminalOptions,
    TargetElement as TerminalTarget,
};

struct WalletCli {
    term: Arc<Mutex<Option<Arc<Terminal>>>>,
    wallet: Arc<Wallet>,
}

impl workflow_log::Sink for WalletCli {
    fn write(&self, _target: Option<&str>, _level: Level, args: &std::fmt::Arguments<'_>) -> bool {
        if let Some(term) = self.term() {
            term.writeln(args.to_string());
            true
        } else {
            false
        }
    }
}

impl WalletCli {
    fn new(wallet: Arc<Wallet>) -> Self {
        WalletCli { term: Arc::new(Mutex::new(None)), wallet }
    }

    fn term(&self) -> Option<Arc<Terminal>> {
        self.term.lock().unwrap().as_ref().cloned() //map(|term| term.clone())
    }

    async fn action(&self, action: Action, argv: Vec<String>, term: Arc<Terminal>) -> Result<()> {
        match action {
            Action::Help => {
                term.writeln("\n\rCommands:\n\r");
                display_help(&term);
            }
            Action::Exit => {
                term.writeln("bye!");
                term.exit();
            }
            Action::GetInfo => {
                //log_trace!("testing 123");
                // let msg = argv[1..].join(" ");
                let response = self.wallet.info().await?;
                term.writeln(response);
            }
            Action::Ping => {
                let msg = argv[1..].join(" ");
                let response = self.wallet.ping(msg).await?;
                term.writeln(response);
            }
            Action::Balance => {
                self.wallet.balance().await?;
            }
            Action::Create => {
                self.wallet.create().await?;
            }
            Action::Broadcast => {
                self.wallet.broadcast().await?;
            }
            Action::CreateUnsignedTx => {
                self.wallet.create_unsigned_transaction().await?;
            }
            Action::DumpUnencrypted => {
                self.wallet.dump_unencrypted().await?;
            }
            Action::NewAddress => {
                let response = self.wallet.new_address().await?;
                term.writeln(response);
            }
            Action::Parse => {
                self.wallet.parse().await?;
            }
            Action::Send => {
                self.wallet.send().await?;
            }
            Action::ShowAddress => {
                self.wallet.show_address().await?;
            }
            Action::Sign => {
                self.wallet.sign().await?;
            }
            Action::Sweep => {
                self.wallet.sweep().await?;
            }
            Action::SubscribeDaaScore => {
                let listener = self.wallet.subscribe_daa_score().await?;
                workflow_core::task::spawn(async move{
                    let term = term;
                    let channel = listener.recv_channel;
                    while let Ok(notification) = channel.recv().await{
                        log_trace!("DAA notification: {:?}", notification);
                        //sender.send(notification)
                        term.writeln(format!("{notification:#?}").replace("\n", "\n\r"));
                    }
                });
                
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Cli for WalletCli {
    fn init(&self, term: &Arc<Terminal>) -> TerminalResult<()> {
        *self.term.lock().unwrap() = Some(term.clone());
        Ok(())
    }

    async fn digest(&self, term: Arc<Terminal>, cmd: String) -> TerminalResult<()> {
        let argv = parse(&cmd);
        let action: Action = argv[0].as_str().try_into()?;
        self.action(action, argv, term).await?;
        Ok(())
    }

    async fn complete(&self, _term: Arc<Terminal>, _cmd: String) -> TerminalResult<Vec<String>> {
        // TODO
        // let argv = parse(&cmd);
        Ok(vec![])
        // if argv.len() == 1 {
        //     // let part = argv.first().unwrap().as_str();
        //     // let mut list = vec![];
        //     // for (cmd,_) in HELP.iter() {
        //     //     if cmd.starts_with(part) {
        //     //         list.push(cmd.to_string());
        //     //     }
        //     // };
        //     // Ok(list)
        //     Ok(vec![])
        // } else {
        //     Ok(vec![])
        // }
    }
}

impl WalletCli {}

pub async fn kaspa_wallet_cli(options: TerminalOptions) -> Result<()> {
    let wallet = Arc::new(Wallet::try_new().await?);

    let cli = Arc::new(WalletCli::new(wallet.clone()));
    let term = Arc::new(Terminal::try_new_with_options(cli.clone(), options)?);
    term.init().await?;

    #[cfg(not(target_arch="wasm32"))]
    workflow_log::pipe(Some(cli.clone()));

    term.writeln("Kaspa Cli Wallet (type 'help' for list of commands)");
    wallet.start().await?;
    term.run().await?;

    Ok(())
}
