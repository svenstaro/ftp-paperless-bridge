use std::env;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_tempfile::TempFile;
use async_trait::async_trait;
use clap::Parser;
use color_eyre::eyre::Result;
use libunftp::{
    auth::{AuthenticationError, Authenticator, Credentials, UserDetail},
    storage::{
        Error as StorageError, ErrorKind::LocalError, Fileinfo, Metadata, Result as StorageResult,
        StorageBackend,
    },
};
use log::{debug, error, info, warn};
use paperless_ngx_api::client::{PaperlessNgxClient, PaperlessNgxClientBuilder};
use tokio::io::AsyncSeekExt;
use tokio::time::{Instant, sleep};

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
    paperless_client: Arc<PaperlessNgxClient>,
}

impl std::fmt::Debug for PaperlessStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "opaque")
    }
}

impl PaperlessStorage {
    pub fn new(paperless_client: Arc<PaperlessNgxClient>) -> Self {
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
impl StorageBackend<User> for PaperlessStorage {
    type Metadata = Meta;

    async fn metadata<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &User,
        _path: P,
    ) -> StorageResult<Self::Metadata> {
        unimplemented!()
    }

    async fn list<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &User,
        _path: P,
    ) -> StorageResult<Vec<Fileinfo<PathBuf, Self::Metadata>>>
    where
        <Self as StorageBackend<User>>::Metadata: Metadata,
    {
        unimplemented!()
    }

    async fn get<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &User,
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
        _user: &User,
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
        let path = tempfile.file_path().to_str().unwrap().to_owned();
        debug!("Saving upload to {path}");

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
        // The upload returns immediately and gives us a Task that we'll have to poll.
        let task = match self.paperless_client.upload(&path).await {
            Ok(task) => task,
            Err(e) => {
                error!("{e}");
                return Err(StorageError::new(LocalError, e));
            }
        };

        let now = Instant::now();
        loop {
            sleep(Duration::from_secs(1)).await;
            let task_status = task.status().await.unwrap();

            debug!("Task status: {task_status:#?}");

            if task_status.status == "STARTED" {
                info!("File uploaded Successfully");
                break;
            }

            if task_status.status == "FAILURE" || task_status.status == "REVOKED" {
                error!("Upload failed");
                break;
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
        _user: &User,
        _path: P,
    ) -> StorageResult<()> {
        unimplemented!()
    }

    async fn mkd<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &User,
        _path: P,
    ) -> StorageResult<()> {
        unimplemented!()
    }

    async fn rename<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &User,
        _from: P,
        _to: P,
    ) -> StorageResult<()> {
        unimplemented!()
    }

    async fn rmd<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &User,
        _path: P,
    ) -> StorageResult<()> {
        unimplemented!()
    }

    async fn cwd<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &User,
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

impl Authenticator<User> for UsernamePasswordAuthenticator {
    async fn authenticate(
        &self,
        username: &str,
        creds: &Credentials,
    ) -> Result<User, AuthenticationError> {
        if let Some(ref password) = creds.password {
            if *password != self.password {
                warn!("Provided password doesn't match");
                return Err(AuthenticationError::BadPassword);
            }
        }
        if username != self.username {
            warn!("Provided username doesn't match");
            return Err(AuthenticationError::BadUser);
        }
        info!("Successfully authenticated");
        Ok(User {})
    }
}

#[derive(Debug)]
struct User;

impl UserDetail for User {}

impl std::fmt::Display for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "User")
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

    // Verify our config works by fetching some docs.
    let paperless_client = Arc::new(
        PaperlessNgxClientBuilder::default()
            .set_url(&args.paperless_url)
            .set_auth_token(&args.paperless_api_token)
            .build()?,
    );
    paperless_client.documents(None).await?;

    let authenticator = Arc::new(UsernamePasswordAuthenticator::new(
        args.username,
        args.password,
    ));

    let paperless_storage = Box::new(move || PaperlessStorage::new(Arc::clone(&paperless_client)));

    info!("Starting FTP server at {}", args.listen);
    let ftp_server = libunftp::ServerBuilder::with_authenticator(paperless_storage, authenticator)
        .greeting("ftp-paperless-bridge")
        .build()?;
    ftp_server.listen(args.listen).await?;

    Ok(())
}
