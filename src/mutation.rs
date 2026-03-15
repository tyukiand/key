pub struct MutationToken(());

impl MutationToken {
    pub fn acquire(read_only: bool) -> anyhow::Result<Self> {
        if read_only {
            anyhow::bail!("Cannot mutate state in --read-only mode");
        }
        Ok(MutationToken(()))
    }
}
