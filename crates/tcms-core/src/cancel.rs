#[derive(Clone)]
pub struct Cancellation(tokio::sync::watch::Sender<bool>);
impl Default for Cancellation {
    fn default() -> Self {
        Self(tokio::sync::watch::channel(false).0)
    }
}
impl Cancellation {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }
    pub async fn cancelled(&self) {
        let _ = self.0.subscribe().wait_for(|v| *v).await;
    }
}
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn sticky() {
        let c = super::Cancellation::default();
        c.cancel();
        tokio::time::timeout(std::time::Duration::from_millis(50), c.cancelled())
            .await
            .unwrap();
    }
}
