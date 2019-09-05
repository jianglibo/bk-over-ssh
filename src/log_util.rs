use fern::colors::{Color, ColoredLevelConfig};
use std::fs;
use std::path::Path;

pub fn setup_logger_empty() {
    setup_test_logger_only_self(Vec::<String>::new());
}

pub fn setup_test_logger<T, I>(verbose_modules: T, other_modules: T)
where
    I: AsRef<str>,
    T: IntoIterator<Item = I>,
{
    if let Err(err) = setup_logger_detail(true, "output.log", verbose_modules, Some(other_modules)) {
        println!("{:?}", err);
    }
}

pub fn setup_test_logger_only_self<T, I>(verbose_modules: T)
where
    I: AsRef<str>,
    T: IntoIterator<Item = I>,
{
    if let Err(err) = setup_logger_detail(true, "output.log", verbose_modules, None) {
        println!("{:?}", err);
    }    
}

#[allow(dead_code)]
pub fn setup_logger_for_this_app<T, I>(
    console: bool,
    log_file_name: &str,
    verbose_modules: T,
) -> Result<(), fern::InitError>
where
    I: AsRef<str>,
    T: IntoIterator<Item = I>,
{
    setup_logger_detail(console, log_file_name, verbose_modules, None)
}

#[allow(dead_code)]
pub fn setup_logger_detail<T, I>(
    console: bool,
    log_file_name: &str,
    verbose_modules: T,
    other_modules: Option<T>,
) -> Result<(), fern::InitError>
where
    I: AsRef<str>,
    T: IntoIterator<Item = I>,
{
    let mut base_config = fern::Dispatch::new().level(log::LevelFilter::Info);

    for module_name in verbose_modules {
        base_config = base_config.level_for(
            format!("bk_over_ssh::{}", module_name.as_ref()),
            log::LevelFilter::Trace,
        );
    }

    if let Some(om) = other_modules {
        for module_name in om {
            base_config =
                base_config.level_for(module_name.as_ref().to_string(), log::LevelFilter::Trace);
        }
    }

    if console {
    let colors = ColoredLevelConfig::new().info(Color::Green);
    let std_out_config = fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "[{}][{}] {}",
                record.target(),
                colors.color(record.level()),
                message
            ))
        })
        .chain(std::io::stdout());
        base_config = base_config.chain(std_out_config);
    }

    let path = Path::new(log_file_name);
    if path.exists() && path.is_file() {
        if let Err(err) = fs::remove_file(log_file_name) {
            println!("remove old log file failed: {:?}, {:?}", log_file_name, err);
        }
    }

    let file_config = fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "[{}][{}] {}",
                // chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .chain(fern::log_file(log_file_name)?);
    
    base_config = base_config.chain(file_config);
    base_config.apply()?;
    Ok(())
}
