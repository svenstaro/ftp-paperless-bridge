use std::env;
use std::fmt::Debug;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_tempfile::TempFile;
use async_trait::async_trait;
use clap::Parser;
use color_eyre::eyre::{Result, eyre};
use libunftp::options::ActivePassiveMode;
use log::{debug, error, info, warn};
use paperless_api::PaperlessClient;
use paperless_api::task::TaskStatus;
use tokio::io::AsyncSeekExt;
use tokio::time::{Instant, sleep};
use unftp_core::{
    auth::{AuthenticationError, Authenticator, Credentials, DefaultUser, Principal},
    storage::{
        Error as StorageError, ErrorKind::LocalError, Fileinfo, Metadata, Result as StorageResult,
        StorageBackend,
    },
};

fn parse_port_range(src: &str) -> Result<RangeInclusive<u16>, String> {
    let parts: Vec<_> = src.split("-").collect();

    if parts.len() != 2 {
        return Err("Wrong format for port range, should be in the format 2222-3333".to_string());
    }

    let range_start: u16 = parts[0]
        .parse()
        .map_err(|_| "First number of port range can't be parsed")?;
    let range_end: u16 = parts[1]
        .parse()
        .map_err(|_| "Second number of port range can't be parsed")?;

    Ok(range_start..=range_end)
}

/// The FTP server part enables both active mode and passive mode at the same time for better
/// flexibility.
#[derive(Parser)]
#[command(name = "ftp-paperless-bridge", author, about, version)]
pub struct CliArgs {
    /// Be verbose
    #[arg(short, long, env = "FTP_PAPERLESS_BRIDGE_VERBOSE")]
    pub verbose: bool,

    /// Listen address
    ///
    /// e.g. 0.0.0.0:2121
    #[arg(short, long, env = "FTP_PAPERLESS_BRIDGE_LISTEN")]
    pub listen: String,

    /// Passive mode port range
    ///
    /// e.g. 2122-2124
    #[arg(long, env = "FTP_PAPERLESS_BRIDGE_PASSIVE_MODE_PORTS", value_parser = parse_port_range)]
    pub passive_mode_ports: RangeInclusive<u16>,

    /// FTP username
    #[arg(short, long, env = "FTP_PAPERLESS_BRIDGE_USERNAME")]
    pub username: String,

    /// FTP password
    #[arg(short, long, env = "FTP_PAPERLESS_BRIDGE_PASSWORD")]
    pub password: String,

    /// URL to your paperless instance
    ///
    /// e.g. https://paperless.example.com
    #[arg(long, env = "FTP_PAPERLESS_BRIDGE_PAPERLESS_URL")]
    pub paperless_url: String,

    /// Paperless API token
    #[arg(long, env = "FTP_PAPERLESS_BRIDGE_PAPERLESS_API_TOKEN")]
    pub paperless_api_token: String,
}

struct PaperlessStorage {
    paperless_client: Arc<PaperlessClient>,
}

impl std::fmt::Debug for PaperlessStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "opaque")
    }
}

impl PaperlessStorage {
    pub fn new(paperless_client: Arc<PaperlessClient>) -> Self {
        Self { paperless_client }
    }
}

#[derive(Debug)]
struct Meta;

impl Metadata for Meta {
    fn len(&self) -> u64 {
        todo!()
    }

    fn is_dir(&self) -> bool {
        todo!()
    }

    fn is_file(&self) -> bool {
        todo!()
    }

    fn is_symlink(&self) -> bool {
        todo!()
    }

    fn modified(&self) -> StorageResult<std::time::SystemTime> {
        todo!()
    }

    fn gid(&self) -> u32 {
        todo!()
    }

    fn uid(&self) -> u32 {
        todo!()
    }
}

#[async_trait]
impl StorageBackend<DefaultUser> for PaperlessStorage {
    type Metadata = Meta;

    async fn metadata<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &DefaultUser,
        _path: P,
    ) -> StorageResult<Self::Metadata> {
        unimplemented!()
    }

    async fn list<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &DefaultUser,
        _path: P,
    ) -> StorageResult<Vec<Fileinfo<PathBuf, Self::Metadata>>>
    where
        <Self as StorageBackend<DefaultUser>>::Metadata: Metadata,
    {
        unimplemented!()
    }

    async fn get<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &DefaultUser,
        _path: P,
        _start_pos: u64,
    ) -> StorageResult<Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin>> {
        unimplemented!()
    }

    async fn put<
        P: AsRef<Path> + Send + Debug,
        R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
    >(
        &self,
        _user: &DefaultUser,
        input: R,
        path: P,
        start_pos: u64,
    ) -> StorageResult<u64> {
        info!("Received upload request");

        // First we'll write the provided file to a temporary location.
        let mut tempfile =
            if let Some(file_name) = path.as_ref().file_name().map(|x| x.to_string_lossy()) {
                TempFile::new_with_name(file_name).await.unwrap()
            } else {
                TempFile::new().await.unwrap()
            };
        let temp_path = tempfile.file_path().to_owned();
        let file_name = temp_path.file_name().unwrap().to_string_lossy().to_string();
        debug!("Saving upload to {}", temp_path.display());

        tempfile.set_len(start_pos).await.unwrap();
        tempfile
            .seek(std::io::SeekFrom::Start(start_pos))
            .await
            .unwrap();

        let mut reader = tokio::io::BufReader::with_capacity(4096, input);
        let mut writer = tokio::io::BufWriter::with_capacity(4096, tempfile);
        let bytes_copied = tokio::io::copy(&mut reader, &mut writer).await?;

        // Now we'll upload the file.
        //
        // The upload returns immediately and gives us a task id that we'll have to poll.
        let task_id = match self
            .paperless_client
            .upload_document(&temp_path, &file_name)
            .await
        {
            Ok(task_id) => task_id,
            Err(e) => {
                error!("{e}");
                return Err(StorageError::new(LocalError, e));
            }
        };

        let now = Instant::now();
        loop {
            sleep(Duration::from_secs(1)).await;
            let tasks = self
                .paperless_client
                .get_task_status(Some(&task_id), None, None)
                .await
                .unwrap();
            let task_status = tasks.first().map(|task| task.status);

            debug!("Task status: {task_status:#?}");

            match task_status {
                Some(TaskStatus::Started | TaskStatus::Success) => {
                    info!("File uploaded Successfully");
                    break;
                }
                Some(TaskStatus::Failure | TaskStatus::Revoked) => {
                    error!("Upload failed");
                    break;
                }
                _ => (),
            }

            // Wait a maximum of 10 seconds.
            if now.elapsed() > Duration::from_secs(10) {
                error!("Timeout during upload");
                break;
            }
        }

        Ok(bytes_copied)
    }

    async fn del<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &DefaultUser,
        _path: P,
    ) -> StorageResult<()> {
        unimplemented!()
    }

    async fn mkd<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &DefaultUser,
        _path: P,
    ) -> StorageResult<()> {
        unimplemented!()
    }

    async fn rename<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &DefaultUser,
        _from: P,
        _to: P,
    ) -> StorageResult<()> {
        unimplemented!()
    }

    async fn rmd<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &DefaultUser,
        _path: P,
    ) -> StorageResult<()> {
        unimplemented!()
    }

    async fn cwd<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &DefaultUser,
        _path: P,
    ) -> StorageResult<()> {
        unimplemented!()
    }
}

#[derive(Debug)]
struct UsernamePasswordAuthenticator {
    username: String,
    password: String,
}

impl UsernamePasswordAuthenticator {
    fn new(username: String, password: String) -> Self {
        Self { username, password }
    }
}

#[async_trait]
impl Authenticator for UsernamePasswordAuthenticator {
    async fn authenticate(
        &self,
        username: &str,
        creds: &Credentials,
    ) -> Result<Principal, AuthenticationError> {
        if let Some(ref password) = creds.password
            && *password != self.password
        {
            warn!("Provided password doesn't match");
            return Err(AuthenticationError::BadPassword);
        }
        if username != self.username {
            warn!("Provided username doesn't match");
            return Err(AuthenticationError::BadUser);
        }
        info!("Successfully authenticated");
        Ok(Principal {
            username: username.to_string(),
        })
    }
}

#[tokio::main]
pub async fn main() -> Result<()> {
    color_eyre::install()?;

    let args = CliArgs::parse();

    unsafe {
        if args.verbose {
            env::set_var("RUST_LOG", "debug");
        } else {
            env::set_var("RUST_LOG", "info");
        }
    }
    env_logger::init();

    // Verify our config works by fetching some server statistics.
    let paperless_client = Arc::new(
        PaperlessClient::new(&args.paperless_url, &args.paperless_api_token, None)
            .map_err(|e| eyre!(e))?,
    );
    paperless_client.get_statistics().await?;

    let authenticator = Arc::new(UsernamePasswordAuthenticator::new(
        args.username,
        args.password,
    ));

    let paperless_storage = Box::new(move || PaperlessStorage::new(Arc::clone(&paperless_client)));

    info!(
        "Starting FTP server at {} with passive port range {}-{}",
        args.listen,
        args.passive_mode_ports.start(),
        args.passive_mode_ports.end()
    );
    let ftp_server = libunftp::ServerBuilder::with_authenticator(paperless_storage, authenticator)
        .greeting("ftp-paperless-bridge")
        .active_passive_mode(ActivePassiveMode::ActiveAndPassive)
        .passive_ports(args.passive_mode_ports)
        .build()?;
    ftp_server.listen(args.listen).await?;

    Ok(())
}
