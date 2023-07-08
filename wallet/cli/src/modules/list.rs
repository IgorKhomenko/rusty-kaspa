use crate::imports::*;

#[derive(Default, Handler)]
#[help("List wallet accounts and their balances")]
pub struct List;

impl List {
    async fn main(self: Arc<Self>, ctx: &Arc<dyn Context>, _argv: Vec<String>, _cmd: &str) -> Result<()> {
        let ctx = ctx.clone().downcast_arc::<WalletCli>()?;

        tprintln!(ctx);

        let mut keys = ctx.wallet().keys().await?;
        while let Some(key) = keys.try_next().await? {
            tprintln!(ctx, "• pk{key}");
            let mut accounts = ctx.wallet().accounts(Some(key.id)).await?;
            while let Some(account) = accounts.try_next().await? {
                tprintln!(ctx, "    {}", account.get_list_string()?);
            }

            tprintln!(ctx);
        }

        Ok(())
    }
}
