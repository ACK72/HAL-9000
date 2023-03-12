use serenity::builder::CreateApplicationCommand;
use serenity::model::prelude::interaction::application_command::CommandDataOption;

pub fn run(_options: &[CommandDataOption]) -> String {
	"Good afternoon, gentlemen. I am a HAL 9000 computer.".to_string()
}

pub fn register(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
	command.name("introduce").description("Make 'BOT' introduce himself")
}