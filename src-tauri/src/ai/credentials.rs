use super::*;
pub trait CredentialStore: Send + Sync {
    fn get(&self) -> Result<Option<String>>;
    fn set(&self, key: &str) -> Result<()>;
    fn delete(&self) -> Result<()>;
}
pub struct WindowsCredentialStore;
#[cfg(windows)]
impl CredentialStore for WindowsCredentialStore {
    fn get(&self) -> Result<Option<String>> {
        match entry()?.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(failure("Não foi possível ler a credencial protegida.")),
        }
    }
    fn set(&self, key: &str) -> Result<()> {
        entry()?
            .set_password(key)
            .map_err(|_| failure("Não foi possível guardar a credencial protegida."))
    }
    fn delete(&self) -> Result<()> {
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(failure("Não foi possível remover a credencial protegida.")),
        }
    }
}
#[cfg(windows)]
fn entry() -> Result<keyring::Entry> {
    keyring::Entry::new("language-assistant-tutor", "opencode-api")
        .map_err(|_| failure("Armazenamento protegido indisponível."))
}
#[cfg(not(windows))]
impl CredentialStore for WindowsCredentialStore {
    fn get(&self) -> Result<Option<String>> {
        Err(failure("Credenciais protegidas requerem Windows."))
    }
    fn set(&self, _: &str) -> Result<()> {
        Err(failure("Credenciais protegidas requerem Windows."))
    }
    fn delete(&self) -> Result<()> {
        Err(failure("Credenciais protegidas requerem Windows."))
    }
}
