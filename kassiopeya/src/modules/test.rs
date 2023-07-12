use crate::imports::*;

#[derive(Default, Handler)]
#[help("Testing ...")]
pub struct Test;

impl Test {
    async fn main(self: Arc<Self>, ctx: &Arc<dyn Context>, _argv: Vec<String>, _cmd: &str) -> Result<()> {
        tprintln!(ctx, "testing...");

        let mut theme = Theme::default();
        theme.foreground = Some("red".to_string());
        ctx.term().set_theme(theme)?;

        Ok(())
    }
}
