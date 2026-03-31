use std::path::PathBuf;

use crate::bridge::IntentExecutor;
use crate::ApprovalPolicy;

pub struct BridgeContext<'a> {
    session_store_path: Option<&'a PathBuf>,
    approval_policy: &'a ApprovalPolicy,
    executor: IntentExecutor,
}

impl<'a> BridgeContext<'a> {
    pub fn new(
        session_store_path: Option<&'a PathBuf>,
        approval_policy: &'a ApprovalPolicy,
        executor: IntentExecutor,
    ) -> Self {
        Self {
            session_store_path,
            approval_policy,
            executor,
        }
    }

    pub fn session_store_path(&self) -> Option<&'a PathBuf> {
        self.session_store_path
    }

    pub fn approval_policy(&self) -> &'a ApprovalPolicy {
        self.approval_policy
    }

    pub fn executor(&self) -> IntentExecutor {
        self.executor
    }
}