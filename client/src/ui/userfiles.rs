use directories::ProjectDirs;
use std::fs;

pub fn write_user_file(filename: &str, content: &str) -> std::io::Result<()> {
    let dirs = ProjectDirs::from("dev", "seagullcode", "FeatherChat")
        .ok_or(std::io::Error::other("Could not find data directory"))?;

    fs::create_dir_all(dirs.data_dir())?;

    fs::write(dirs.data_dir().join(filename), content)
}

pub fn read_user_file(filename: &str) -> std::io::Result<Option<String>> {
    let dirs = ProjectDirs::from("dev", "SeagullCode", "FeatherChat")
        .ok_or(std::io::Error::other("Could not find data directory"))?;

    let path = dirs.data_dir().join(filename);

    if path.exists() {
        Ok(Some(fs::read_to_string(path)?))
    } else {
        Ok(None)
    }
}
