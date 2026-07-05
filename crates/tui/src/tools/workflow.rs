//! Model-facing Workflow runner over the live sub-agent runtime.
//!
//! The JS VM stays in `codewhale-whaleflow-js`; this module supplies the TUI
//! driver that turns each `task(...)` call into a real `SubAgentManager` spawn.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use codewhale_whaleflow::{
    AgentType, BranchResult, BranchSpec, BudgetSpec, ControlNodeKind, ControlNodeResult,
    IsolationMode, LeafResult, LeafSpec, ReduceSpec, SequenceSpec, TaskMode,
    WorkflowExecution as IrWorkflowExecution, WorkflowMemoUsage, WorkflowNode,
    WorkflowRunStatus as IrWorkflowRunStatus, WorkflowSpec, WorkflowUsage,
    compile_javascript_workflow, compile_typescript_workflow,
};
use codewhale_whaleflow_js::{
    BudgetSnapshot, DriverError, ProgressEvent, SpawnedTask, TaskCompletion, TaskRequest,
    WhaleflowVm, WorkflowDriver,
};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_bool, optional_str, optional_u64,
};
use crate::tools::subagent::{
    SharedSubAgentManager, SubAgentCompletion, SubAgentRuntime, SubAgentStatus, spawn_workflow_task,
};
use crate::utils::spawn_supervised;

#[derive(Clone)]
pub struct WorkflowTool {
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
    runs: SharedWorkflowRuns,
    controllers: SharedWorkflowControllers,
}

impl WorkflowTool {
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, runtime: SubAgentRuntime) -> Self {
        Self {
            manager,
            runtime,
            runs: Arc::new(Mutex::new(HashMap::new())),
            controllers: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

type SharedWorkflowRuns = Arc<Mutex<HashMap<String, WorkflowRunRecord>>>;
type SharedWorkflowControllers = Arc<Mutex<HashMap<String, Arc<SubAgentWorkflowDriver>>>>;

#[derive(Debug, Clone, Serialize)]
struct WorkflowRunRecord {
    run_id: String,
    status: WorkflowRunStatus,
    started_at_ms: u64,
    completed_at_ms: Option<u64>,
    source_path: Option<PathBuf>,
    workflow_id: Option<String>,
    workflow_goal: Option<String>,
    token_budget: Option<u64>,
    child_ids: Vec<String>,
    progress: Vec<String>,
    result: Option<Value>,
    execution: Option<IrWorkflowExecution>,
    error: Option<String>,
}

impl WorkflowRunRecord {
    fn new(
        run_id: String,
        source_path: Option<PathBuf>,
        token_budget: Option<u64>,
        spec: Option<&WorkflowSpec>,
    ) -> Self {
        Self {
            run_id,
            status: WorkflowRunStatus::Running,
            started_at_ms: now_ms(),
            completed_at_ms: None,
            source_path,
            workflow_id: spec.and_then(|spec| spec.id.clone()),
            workflow_goal: spec.map(|spec| spec.goal.clone()),
            token_budget,
            child_ids: Vec::new(),
            progress: Vec::new(),
            result: None,
            execution: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkflowRunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkflowAction {
    Start,
    Run,
    Status,
    Cancel,
}

fn parse_workflow_action(input: &Value) -> Result<WorkflowAction, ToolError> {
    let Some(action) = optional_str(input, "action") else {
        return Ok(WorkflowAction::Start);
    };
    match action.trim().to_ascii_lowercase().as_str() {
        "" | "start" | "spawn" => Ok(WorkflowAction::Start),
        "run" | "wait" => Ok(WorkflowAction::Run),
        "status" | "list" | "inspect" => Ok(WorkflowAction::Status),
        "cancel" | "stop" | "abort" => Ok(WorkflowAction::Cancel),
        other => Err(ToolError::invalid_input(format!(
            "Invalid workflow action '{other}'. Use start, run, status, or cancel."
        ))),
    }
}

#[async_trait]
impl ToolSpec for WorkflowTool {
    fn name(&self) -> &'static str {
        "workflow"
    }

    fn description(&self) -> &'static str {
        concat!(
            "Start, run, inspect, or cancel a Workflow. Workflows execute deterministic JS with args, phase/log progress, and task(...) calls that dispatch real sub-agents through Fleet/sub-agent scheduling. ",
            "Use action=start for detached orchestration and action=status with run_id to inspect progress. Use action=run when the model needs the final result before continuing."
        )
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "run", "status", "cancel"],
                    "description": "start (default) launches a Workflow in the background. run waits for completion. status lists runs or inspects run_id. cancel stops a run and its child agents."
                },
                "run_id": {
                    "type": "string",
                    "description": "Workflow run id for action=status or action=cancel."
                },
                "script": {
                    "type": "string",
                    "description": "Workflow JS source. The runtime provides args, task(...), parallel(...), pipeline(...), log(...), phase(...), and budget."
                },
                "source_path": {
                    "type": "string",
                    "description": "Path to a .workflow.js script inside the workspace. Use instead of script for checked-in workflows."
                },
                "args": {
                    "description": "JSON value exposed to the script as args. Defaults to null."
                },
                "token_budget": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Optional shared Workflow budget hint and default child token budget ceiling."
                },
                "wait": {
                    "type": "boolean",
                    "description": "For action=start, wait for completion instead of returning immediately."
                }
            },
            "required": [],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    fn approval_requirement_for(&self, input: &Value) -> ApprovalRequirement {
        match parse_workflow_action(input) {
            Ok(WorkflowAction::Status) => ApprovalRequirement::Auto,
            _ => ApprovalRequirement::Required,
        }
    }

    fn starts_detached_for(&self, input: &Value) -> bool {
        matches!(parse_workflow_action(input), Ok(WorkflowAction::Start))
            && !optional_bool(input, "wait", false)
    }

    fn supports_parallel_for(&self, input: &Value) -> bool {
        matches!(parse_workflow_action(input), Ok(WorkflowAction::Status))
    }

    fn is_read_only_for(&self, input: &Value) -> bool {
        matches!(parse_workflow_action(input), Ok(WorkflowAction::Status))
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        match parse_workflow_action(&input)? {
            WorkflowAction::Start => {
                let wait = optional_bool(&input, "wait", false);
                start_workflow(
                    input,
                    context,
                    self.manager.clone(),
                    self.runtime.clone(),
                    self.runs.clone(),
                    self.controllers.clone(),
                    wait,
                )
                .await
            }
            WorkflowAction::Run => {
                start_workflow(
                    input,
                    context,
                    self.manager.clone(),
                    self.runtime.clone(),
                    self.runs.clone(),
                    self.controllers.clone(),
                    true,
                )
                .await
            }
            WorkflowAction::Status => status_workflow(input, self.runs.clone()),
            WorkflowAction::Cancel => {
                cancel_workflow(input, self.runs.clone(), self.controllers.clone()).await
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn start_workflow(
    input: Value,
    context: &ToolContext,
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
    runs: SharedWorkflowRuns,
    controllers: SharedWorkflowControllers,
    wait: bool,
) -> Result<ToolResult, ToolError> {
    let source = workflow_source(&input, context)?;
    let args = input.get("args").cloned().unwrap_or(Value::Null);
    let token_budget = optional_u64(&input, "token_budget", 0);
    let token_budget = (token_budget > 0).then_some(token_budget);
    let run_id = format!("workflow_{}", &Uuid::new_v4().to_string()[..8]);

    {
        let mut runs_guard = lock_mutex(&runs)?;
        runs_guard.insert(
            run_id.clone(),
            WorkflowRunRecord::new(
                run_id.clone(),
                source.path.clone(),
                token_budget,
                source.spec.as_ref(),
            ),
        );
    }

    let driver =
        SubAgentWorkflowDriver::new(run_id.clone(), manager, runtime, runs.clone(), token_budget);
    {
        let mut controllers_guard = lock_mutex(&controllers)?;
        controllers_guard.insert(run_id.clone(), driver.clone());
    }

    let run = run_workflow_vm(
        run_id.clone(),
        source.source,
        source.spec,
        args,
        driver,
        runs.clone(),
        controllers.clone(),
    );
    if wait {
        run.await;
    } else {
        spawn_supervised("workflow-run", std::panic::Location::caller(), run);
    }

    workflow_result_for(&run_id, runs)
}

fn status_workflow(input: Value, runs: SharedWorkflowRuns) -> Result<ToolResult, ToolError> {
    if let Some(run_id) = optional_str(&input, "run_id") {
        return workflow_result_for(run_id, runs);
    }
    let mut records = {
        let runs_guard = lock_mutex(&runs)?;
        runs_guard.values().cloned().collect::<Vec<_>>()
    };
    records.sort_by_key(|record| record.started_at_ms);
    ToolResult::json(&json!({
        "action": "status",
        "count": records.len(),
        "runs": records,
    }))
    .map_err(|err| ToolError::execution_failed(err.to_string()))
}

async fn cancel_workflow(
    input: Value,
    runs: SharedWorkflowRuns,
    controllers: SharedWorkflowControllers,
) -> Result<ToolResult, ToolError> {
    let run_id =
        optional_str(&input, "run_id").ok_or_else(|| ToolError::missing_field("run_id"))?;
    let controller = {
        let mut controllers_guard = lock_mutex(&controllers)?;
        controllers_guard.remove(run_id)
    };
    if let Some(controller) = controller {
        controller.force_cancel_all();
    }
    {
        let mut runs_guard = lock_mutex(&runs)?;
        let record = runs_guard.get_mut(run_id).ok_or_else(|| {
            ToolError::invalid_input(format!("Unknown workflow run_id '{run_id}'"))
        })?;
        record.status = WorkflowRunStatus::Cancelled;
        record.completed_at_ms = Some(now_ms());
        record.error = Some("cancelled by workflow tool".to_string());
    }
    workflow_result_for(run_id, runs)
}

async fn run_workflow_vm(
    run_id: String,
    source: String,
    spec: Option<WorkflowSpec>,
    args: Value,
    driver: Arc<SubAgentWorkflowDriver>,
    runs: SharedWorkflowRuns,
    controllers: SharedWorkflowControllers,
) {
    let result = WhaleflowVm::new()
        .run_script(&source, args, driver.clone())
        .await;
    let mut status = WorkflowRunStatus::Completed;
    let mut output = None;
    let mut error = None;
    match result {
        Ok(value) => output = Some(value),
        Err(err) => {
            status = WorkflowRunStatus::Failed;
            error = Some(err.to_string());
        }
    }
    if let Ok(mut runs_guard) = runs.lock() {
        if let Some(record) = runs_guard.get_mut(&run_id) {
            if record.status != WorkflowRunStatus::Cancelled {
                record.status = status;
                record.result = output;
                record.error = error;
                record.execution = spec.as_ref().map(|spec| {
                    execution_from_declarative_spec(spec, driver.task_records_snapshot(), status)
                });
                record.completed_at_ms = Some(now_ms());
            }
        }
    }
    if let Ok(mut controllers_guard) = controllers.lock() {
        controllers_guard.remove(&run_id);
    }
}

fn workflow_result_for(run_id: &str, runs: SharedWorkflowRuns) -> Result<ToolResult, ToolError> {
    let record = {
        let runs_guard = lock_mutex(&runs)?;
        runs_guard.get(run_id).cloned().ok_or_else(|| {
            ToolError::invalid_input(format!("Unknown workflow run_id '{run_id}'"))
        })?
    };
    let mut result =
        ToolResult::json(&record).map_err(|err| ToolError::execution_failed(err.to_string()))?;
    result.metadata = Some(json!({
        "run_id": record.run_id,
        "status": record.status,
        "terminal": record.status != WorkflowRunStatus::Running,
        "child_count": record.child_ids.len(),
    }));
    Ok(result)
}

#[derive(Debug)]
struct WorkflowSource {
    source: String,
    path: Option<PathBuf>,
    spec: Option<WorkflowSpec>,
}

fn workflow_source(input: &Value, context: &ToolContext) -> Result<WorkflowSource, ToolError> {
    let script = optional_str(input, "script")
        .or_else(|| optional_str(input, "source"))
        .map(str::to_string);
    let source_path = optional_str(input, "source_path").or_else(|| optional_str(input, "path"));
    match (script, source_path) {
        (Some(source), None) if !source.trim().is_empty() => workflow_source_from_raw(source, None),
        (None, Some(path)) => read_workflow_source_path(path, context),
        (Some(_), Some(_)) => Err(ToolError::invalid_input(
            "Use either script or source_path, not both",
        )),
        _ => Err(ToolError::missing_field("script")),
    }
}

fn read_workflow_source_path(
    path: &str,
    context: &ToolContext,
) -> Result<WorkflowSource, ToolError> {
    let raw = Path::new(path);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        context.workspace.join(raw)
    };
    let canonical = joined.canonicalize().map_err(|err| {
        ToolError::invalid_input(format!(
            "Failed to resolve workflow source_path '{path}': {err}"
        ))
    })?;
    if !context.trust_mode {
        let workspace = context
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| context.workspace.clone());
        if !canonical.starts_with(&workspace) {
            return Err(ToolError::permission_denied(format!(
                "workflow source_path must stay inside the workspace: {}",
                canonical.display()
            )));
        }
    }
    let source = std::fs::read_to_string(&canonical).map_err(|err| {
        ToolError::execution_failed(format!(
            "Failed to read workflow source_path '{}': {err}",
            canonical.display()
        ))
    })?;
    workflow_source_from_raw(source, Some(canonical))
}

fn workflow_source_from_raw(
    source: String,
    path: Option<PathBuf>,
) -> Result<WorkflowSource, ToolError> {
    let adapted = adapt_workflow_source(&source, path.as_deref())?;
    Ok(WorkflowSource {
        source: adapted.source,
        path,
        spec: adapted.spec,
    })
}

struct AdaptedWorkflowSource {
    source: String,
    spec: Option<WorkflowSpec>,
}

fn adapt_workflow_source(
    source: &str,
    path: Option<&Path>,
) -> Result<AdaptedWorkflowSource, ToolError> {
    if !looks_like_declarative_workflow(source) {
        return Ok(AdaptedWorkflowSource {
            source: source.to_string(),
            spec: None,
        });
    }

    let identifier = path
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<inline workflow>".to_string());
    let extension = path
        .and_then(Path::extension)
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    let spec = if extension.eq_ignore_ascii_case("ts") {
        compile_typescript_workflow(&identifier, source)
    } else {
        compile_javascript_workflow(&identifier, source)
    }
    .map_err(|err| {
        ToolError::invalid_input(format!(
            "Failed to compile declarative Workflow source '{identifier}': {err}"
        ))
    })?;

    let lowered = lower_declarative_workflow_to_imperative_js(&spec)?;
    Ok(AdaptedWorkflowSource {
        source: lowered,
        spec: Some(spec),
    })
}

fn looks_like_declarative_workflow(source: &str) -> bool {
    let trimmed = source.trim_start();
    trimmed.starts_with("workflow(")
        || trimmed.starts_with("export default workflow(")
        || source.contains("\nworkflow(")
        || source.contains("\nexport default workflow(")
}

fn lower_declarative_workflow_to_imperative_js(spec: &WorkflowSpec) -> Result<String, ToolError> {
    let mut lowerer = DeclarativeWorkflowLowerer::default();
    lowerer.line("\"use strict\";");
    lowerer.line("const __results = {};");
    lowerer.line(format!(
        "phase({});",
        js_string(&format!("workflow: {}", spec.goal))
    ));
    for node in &spec.nodes {
        lowerer.lower_node(node, None)?;
    }
    lowerer.line("return __results;");
    Ok(lowerer.finish())
}

#[derive(Default)]
struct DeclarativeWorkflowLowerer {
    source: String,
    next_var: usize,
}

impl DeclarativeWorkflowLowerer {
    fn finish(self) -> String {
        self.source
    }

    fn line(&mut self, line: impl AsRef<str>) {
        self.source.push_str(line.as_ref());
        self.source.push('\n');
    }

    fn next_temp(&mut self, prefix: &str) -> String {
        let value = format!("__{prefix}_{}", self.next_var);
        self.next_var += 1;
        value
    }

    fn lower_node(&mut self, node: &WorkflowNode, phase: Option<&str>) -> Result<(), ToolError> {
        match node {
            WorkflowNode::Leaf(spec) => self.lower_leaf(spec, phase),
            WorkflowNode::BranchSet(spec) => self.lower_branch(spec),
            WorkflowNode::Sequence(spec) => self.lower_sequence(spec),
            WorkflowNode::Reduce(spec) => self.lower_reduce(spec),
            WorkflowNode::TeacherReview(_) => Err(unsupported_declarative_node("teacher_review")),
            WorkflowNode::LoopUntil(_) => Err(unsupported_declarative_node("loop_until")),
            WorkflowNode::Cond(_) => Err(unsupported_declarative_node("cond")),
            WorkflowNode::Expand(_) => Err(unsupported_declarative_node("expand")),
        }
    }

    fn lower_leaf(&mut self, spec: &LeafSpec, phase: Option<&str>) -> Result<(), ToolError> {
        self.line(format!(
            "__results[{}] = await task({});",
            js_string(&spec.id),
            leaf_task_options_expression(spec, phase)?
        ));
        Ok(())
    }

    fn lower_branch(&mut self, spec: &BranchSpec) -> Result<(), ToolError> {
        self.line(format!("phase({});", js_string(&spec.id)));
        if spec.parallel {
            let mut leaves = Vec::new();
            for child in &spec.children {
                let WorkflowNode::Leaf(leaf) = child else {
                    return Err(ToolError::invalid_input(format!(
                        "Declarative Workflow adapter only supports leaf children inside parallel branch '{}'",
                        spec.id
                    )));
                };
                leaves.push(leaf);
            }
            let temp = self.next_temp("parallel");
            self.line(format!("const {temp} = await Promise.all(["));
            for leaf in &leaves {
                self.line(format!(
                    "  task({}),",
                    leaf_task_options_expression(leaf, Some(&spec.id))?
                ));
            }
            self.line("]);");
            for (index, leaf) in leaves.iter().enumerate() {
                self.line(format!(
                    "__results[{}] = {temp}[{index}];",
                    js_string(&leaf.id)
                ));
            }
            return Ok(());
        }

        for child in &spec.children {
            self.lower_node(child, Some(&spec.id))?;
        }
        Ok(())
    }

    fn lower_sequence(&mut self, spec: &SequenceSpec) -> Result<(), ToolError> {
        self.line(format!("phase({});", js_string(&spec.id)));
        for child in &spec.children {
            self.lower_node(child, Some(&spec.id))?;
        }
        Ok(())
    }

    fn lower_reduce(&mut self, spec: &ReduceSpec) -> Result<(), ToolError> {
        let inputs = result_inputs_expression(&spec.inputs);
        self.line(format!(
            "__results[{}] = await task({});",
            js_string(&spec.id),
            task_options_expression(
                format!(
                    "{} + \"\\n\\nInputs:\\n\" + {inputs}",
                    js_string(&spec.prompt)
                ),
                "general",
                None,
                false,
                None,
                &spec.id,
                Some("reduce"),
                None,
            )
        ));
        Ok(())
    }
}

fn unsupported_declarative_node(kind: &str) -> ToolError {
    ToolError::invalid_input(format!(
        "Declarative Workflow adapter does not yet support {kind} nodes"
    ))
}

fn leaf_description(spec: &LeafSpec) -> String {
    let mut description = spec.prompt.trim().to_string();
    let mut metadata = Vec::new();
    metadata.push(format!("Workflow leaf id: {}", spec.id));
    metadata.push(format!("Mode: {}", task_mode_name(spec.mode)));
    if !spec.file_scope.is_empty() {
        metadata.push(format!("File scope: {}", spec.file_scope.join(", ")));
    }
    if !spec.depends_on_results.is_empty() {
        metadata.push(format!(
            "Depends on results: {}",
            spec.depends_on_results.join(", ")
        ));
    }
    if spec.budget != BudgetSpec::default() {
        let mut budget = Vec::new();
        if let Some(max_steps) = spec.budget.max_steps {
            budget.push(format!("max_steps={max_steps}"));
        }
        if let Some(timeout_secs) = spec.budget.timeout_secs {
            budget.push(format!("timeout_secs={timeout_secs}"));
        }
        if let Some(max_parallel) = spec.budget.max_parallel {
            budget.push(format!("max_parallel={max_parallel}"));
        }
        if let Some(max_tokens) = spec.budget.max_tokens {
            budget.push(format!("max_tokens={max_tokens}"));
        }
        if !budget.is_empty() {
            metadata.push(format!("Budget: {}", budget.join(", ")));
        }
    }
    if !metadata.is_empty() {
        description.push_str("\n\nWorkflow metadata:\n");
        for item in metadata {
            description.push_str("- ");
            description.push_str(&item);
            description.push('\n');
        }
    }
    description
}

fn leaf_task_options_expression(spec: &LeafSpec, phase: Option<&str>) -> Result<String, ToolError> {
    validate_leaf_runtime_contract(spec)?;
    Ok(task_options_expression(
        leaf_description_expression(spec),
        leaf_subagent_type(spec)?,
        spec.profile.as_deref(),
        matches!(spec.isolation, IsolationMode::Worktree),
        spec.budget.max_tokens,
        &spec.id,
        phase,
        leaf_allowed_tools(spec)?,
    ))
}

fn validate_leaf_runtime_contract(spec: &LeafSpec) -> Result<(), ToolError> {
    if spec.mode == TaskMode::ReadOnly && spec.permissions.allow_write {
        return Err(ToolError::invalid_input(format!(
            "Workflow leaf '{}' is read_only but requests allow_write permissions",
            spec.id
        )));
    }
    if spec.mode == TaskMode::ReadOnly && matches!(spec.agent_type, AgentType::Implementer) {
        return Err(ToolError::invalid_input(format!(
            "Workflow leaf '{}' is read_only but uses implementer agent_type",
            spec.id
        )));
    }
    if spec.mode == TaskMode::ReadWrite
        && matches!(
            spec.agent_type,
            AgentType::Explore | AgentType::Plan | AgentType::Review | AgentType::Verifier
        )
    {
        return Err(ToolError::invalid_input(format!(
            "Workflow leaf '{}' is read_write but uses read-only agent_type {}",
            spec.id,
            agent_type_name(spec.agent_type)
        )));
    }
    if spec.mode == TaskMode::ReadOnly
        && spec
            .permissions
            .allowed_tools
            .iter()
            .any(|tool| is_write_or_shell_tool(tool))
    {
        return Err(ToolError::invalid_input(format!(
            "Workflow leaf '{}' is read_only but requests write/shell allowed_tools",
            spec.id
        )));
    }
    Ok(())
}

fn leaf_description_expression(spec: &LeafSpec) -> String {
    let description = js_string(&leaf_description(spec));
    if spec.depends_on_results.is_empty() {
        return description;
    }
    let inputs = result_inputs_expression(&spec.depends_on_results);
    format!("{description} + \"\\n\\nInputs:\\n\" + {inputs}")
}

fn result_inputs_expression(inputs: &[String]) -> String {
    let entries = inputs
        .iter()
        .map(|input| format!("[{}, __results[{}]]", js_string(input), js_string(input)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[{entries}].map(([id, value]) => \"--- \" + id + \" ---\\n\" + String(value ?? \"\")).join(\"\\n\\n\")"
    )
}

fn leaf_subagent_type(spec: &LeafSpec) -> Result<&'static str, ToolError> {
    if spec.mode == TaskMode::ReadOnly && spec.agent_type == AgentType::General {
        return Ok("review");
    }
    Ok(agent_type_name(spec.agent_type))
}

fn leaf_allowed_tools(spec: &LeafSpec) -> Result<Option<Vec<String>>, ToolError> {
    if !spec.permissions.allowed_tools.is_empty() {
        return Ok(Some(spec.permissions.allowed_tools.clone()));
    }
    if spec.mode != TaskMode::ReadOnly {
        return Ok(None);
    }
    Ok(Some(
        read_only_allowed_tools(spec.agent_type)
            .iter()
            .map(|tool| (*tool).to_string())
            .collect(),
    ))
}

fn read_only_allowed_tools(agent_type: AgentType) -> &'static [&'static str] {
    match agent_type {
        AgentType::Verifier => &["list_dir", "read_file", "grep_files", "file_search"],
        _ => &["list_dir", "read_file", "grep_files", "file_search"],
    }
}

fn is_write_or_shell_tool(tool: &str) -> bool {
    matches!(
        tool.trim(),
        "write_file"
            | "edit_file"
            | "apply_patch"
            | "exec_shell"
            | "exec_shell_wait"
            | "exec_shell_interact"
            | "exec_wait"
            | "exec_interact"
    )
}

fn task_options_expression(
    description_expr: String,
    subagent_type: &str,
    profile: Option<&str>,
    worktree: bool,
    token_budget: Option<u64>,
    label: &str,
    phase: Option<&str>,
    allowed_tools: Option<Vec<String>>,
) -> String {
    let mut fields = vec![
        format!("description: {description_expr}"),
        format!("type: {}", js_string(subagent_type)),
        format!("label: {}", js_string(label)),
    ];
    if let Some(phase) = phase {
        fields.push(format!("phase: {}", js_string(phase)));
    }
    if let Some(profile) = profile {
        fields.push(format!("profile: {}", js_string(profile)));
    }
    if worktree {
        fields.push("worktree: true".to_string());
    }
    if let Some(token_budget) = token_budget {
        fields.push(format!("tokenBudget: {token_budget}"));
    }
    if let Some(allowed_tools) = allowed_tools {
        fields.push(format!(
            "allowedTools: {}",
            serde_json::to_string(&allowed_tools).expect("serializing tool names cannot fail")
        ));
    }
    format!("{{ {} }}", fields.join(", "))
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing JS string cannot fail")
}

fn agent_type_name(agent_type: AgentType) -> &'static str {
    match agent_type {
        AgentType::General => "general",
        AgentType::Explore => "explore",
        AgentType::Plan => "plan",
        AgentType::Review => "review",
        AgentType::Implementer => "implementer",
        AgentType::Verifier => "verifier",
    }
}

fn task_mode_name(mode: TaskMode) -> &'static str {
    match mode {
        TaskMode::ReadOnly => "read_only",
        TaskMode::ReadWrite => "read_write",
    }
}

#[derive(Debug, Clone)]
struct RuntimeTaskRecord {
    agent_id: String,
    label: Option<String>,
    status: IrWorkflowRunStatus,
    output: Option<String>,
}

struct SubAgentWorkflowDriver {
    run_id: String,
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
    runs: SharedWorkflowRuns,
    completion_tx: mpsc::UnboundedSender<SubAgentCompletion>,
    completion_state: Arc<Mutex<CompletionState>>,
    child_ids: Arc<Mutex<Vec<String>>>,
    task_records: Arc<Mutex<HashMap<String, RuntimeTaskRecord>>>,
    total_budget: Option<u64>,
    spent_budget: AtomicU64,
}

impl SubAgentWorkflowDriver {
    fn new(
        run_id: String,
        manager: SharedSubAgentManager,
        runtime: SubAgentRuntime,
        runs: SharedWorkflowRuns,
        total_budget: Option<u64>,
    ) -> Arc<Self> {
        let (completion_tx, completion_rx) = mpsc::unbounded_channel();
        let driver = Arc::new(Self {
            run_id,
            manager,
            runtime,
            runs,
            completion_tx,
            completion_state: Arc::new(Mutex::new(CompletionState::default())),
            child_ids: Arc::new(Mutex::new(Vec::new())),
            task_records: Arc::new(Mutex::new(HashMap::new())),
            total_budget,
            spent_budget: AtomicU64::new(0),
        });
        spawn_completion_pump(driver.clone(), completion_rx);
        driver
    }

    fn force_cancel_all(&self) {
        let ids = self
            .child_ids
            .lock()
            .map(|ids| ids.clone())
            .unwrap_or_default();
        cancel_child_agents(self.manager.clone(), ids);
        if let Ok(mut state) = self.completion_state.lock() {
            for (_, waiter) in state.waiters.drain() {
                let _ = waiter.send(TaskCompletion::Cancelled);
            }
        }
    }

    fn record_child(&self, agent_id: &str) {
        if let Ok(mut ids) = self.child_ids.lock() {
            if !ids.iter().any(|id| id == agent_id) {
                ids.push(agent_id.to_string());
            }
        }
        if let Ok(mut runs) = self.runs.lock()
            && let Some(record) = runs.get_mut(&self.run_id)
            && !record.child_ids.iter().any(|id| id == agent_id)
        {
            record.child_ids.push(agent_id.to_string());
        }
    }

    fn record_task_request(&self, agent_id: &str, request: &TaskRequest) {
        if let Ok(mut records) = self.task_records.lock() {
            records.insert(
                agent_id.to_string(),
                RuntimeTaskRecord {
                    agent_id: agent_id.to_string(),
                    label: request.label.clone(),
                    status: IrWorkflowRunStatus::Running,
                    output: None,
                },
            );
        }
        let pending_completion = self
            .completion_state
            .lock()
            .ok()
            .and_then(|state| state.pending.get(agent_id).cloned());
        if let Some(completion) = pending_completion {
            self.record_task_completion(agent_id, &completion);
        }
    }

    fn record_task_completion(&self, agent_id: &str, completion: &TaskCompletion) {
        if let Ok(mut records) = self.task_records.lock()
            && let Some(record) = records.get_mut(agent_id)
        {
            let (status, output) = match completion {
                TaskCompletion::Completed { text } => {
                    (IrWorkflowRunStatus::Succeeded, Some(text.clone()))
                }
                TaskCompletion::Failed { message } => {
                    (IrWorkflowRunStatus::Failed, Some(message.clone()))
                }
                TaskCompletion::Cancelled => (IrWorkflowRunStatus::Cancelled, None),
                TaskCompletion::BudgetExhausted { message } => {
                    (IrWorkflowRunStatus::BudgetExceeded, Some(message.clone()))
                }
            };
            record.status = status;
            record.output = output;
        }
    }

    fn task_records_snapshot(&self) -> Vec<RuntimeTaskRecord> {
        self.task_records
            .lock()
            .map(|records| records.values().cloned().collect())
            .unwrap_or_default()
    }

    fn add_waiter_or_complete(&self, agent_id: String, waiter: oneshot::Sender<TaskCompletion>) {
        let mut state = self
            .completion_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(completion) = state.pending.remove(&agent_id) {
            let _ = waiter.send(completion);
        } else {
            state.waiters.insert(agent_id, waiter);
        }
    }

    fn deliver_completion(&self, agent_id: String, completion: TaskCompletion) {
        self.record_task_completion(&agent_id, &completion);
        let mut state = self
            .completion_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(waiter) = state.waiters.remove(&agent_id) {
            let _ = waiter.send(completion);
        } else {
            state.pending.insert(agent_id, completion);
        }
    }
}

#[derive(Default)]
struct CompletionState {
    waiters: HashMap<String, oneshot::Sender<TaskCompletion>>,
    pending: HashMap<String, TaskCompletion>,
}

#[async_trait]
impl WorkflowDriver for SubAgentWorkflowDriver {
    async fn spawn_task(&self, request: TaskRequest) -> Result<SpawnedTask, DriverError> {
        let runtime = self
            .runtime
            .clone()
            .with_parent_completion_tx(self.completion_tx.clone());
        let request_record = request.clone();
        let result = spawn_workflow_task(request, self.manager.clone(), runtime)
            .await
            .map_err(|err| DriverError::Rejected(err.to_string()))?;
        let task_id = result.agent_id.clone();
        self.record_child(&task_id);
        self.record_task_request(&task_id, &request_record);
        let (tx, rx) = oneshot::channel();
        self.add_waiter_or_complete(task_id.clone(), tx);
        Ok(SpawnedTask {
            task_id,
            completion: rx,
        })
    }

    fn cancel_all(&self) {
        self.force_cancel_all();
    }

    fn budget(&self) -> BudgetSnapshot {
        BudgetSnapshot {
            total: self.total_budget,
            spent: self.spent_budget.load(Ordering::Relaxed),
        }
    }

    fn progress(&self, event: ProgressEvent) {
        let message = match event {
            ProgressEvent::Log { message } => format!("log: {message}"),
            ProgressEvent::Phase { title } => format!("phase: {title}"),
        };
        if let Ok(mut runs) = self.runs.lock()
            && let Some(record) = runs.get_mut(&self.run_id)
        {
            record.progress.push(message);
        }
    }
}

fn execution_from_declarative_spec(
    spec: &WorkflowSpec,
    records: Vec<RuntimeTaskRecord>,
    terminal_status: WorkflowRunStatus,
) -> IrWorkflowExecution {
    let by_label = records
        .into_iter()
        .filter_map(|record| record.label.clone().map(|label| (label, record)))
        .collect::<HashMap<_, _>>();
    let mut execution = IrWorkflowExecution::default();
    for node in &spec.nodes {
        push_execution_node(node, &by_label, &mut execution);
    }
    match terminal_status {
        WorkflowRunStatus::Completed => {}
        WorkflowRunStatus::Failed => mark_ir_status(&mut execution, IrWorkflowRunStatus::Failed),
        WorkflowRunStatus::Cancelled => {
            mark_ir_status(&mut execution, IrWorkflowRunStatus::Cancelled);
        }
        WorkflowRunStatus::Running => {
            execution.status = IrWorkflowRunStatus::Running;
        }
    }
    execution
}

fn push_execution_node(
    node: &WorkflowNode,
    records: &HashMap<String, RuntimeTaskRecord>,
    execution: &mut IrWorkflowExecution,
) {
    match node {
        WorkflowNode::Leaf(spec) => push_leaf_execution(spec, records, execution),
        WorkflowNode::BranchSet(spec) => push_branch_execution(spec, records, execution),
        WorkflowNode::Sequence(spec) => push_sequence_execution(spec, records, execution),
        WorkflowNode::Reduce(spec) => push_control_execution(
            spec.id.as_str(),
            ControlNodeKind::Reduce,
            records.get(&spec.id),
            spec.inputs.clone(),
            Some(spec.prompt.clone()),
            execution,
        ),
        WorkflowNode::TeacherReview(spec) => push_control_execution(
            spec.id.as_str(),
            ControlNodeKind::TeacherReview,
            records.get(&spec.id),
            spec.candidates.clone(),
            Some("teacher review not lowered by the production adapter".to_string()),
            execution,
        ),
        WorkflowNode::LoopUntil(spec) => push_control_execution(
            spec.id.as_str(),
            ControlNodeKind::LoopUntil,
            records.get(&spec.id),
            spec.children.iter().map(declarative_node_id).collect(),
            Some("loop_until not lowered by the production adapter".to_string()),
            execution,
        ),
        WorkflowNode::Cond(spec) => push_control_execution(
            spec.id.as_str(),
            ControlNodeKind::Cond,
            records.get(&spec.id),
            spec.then_nodes
                .iter()
                .chain(spec.else_nodes.iter())
                .map(declarative_node_id)
                .collect(),
            Some("cond not lowered by the production adapter".to_string()),
            execution,
        ),
        WorkflowNode::Expand(spec) => push_control_execution(
            spec.id.as_str(),
            ControlNodeKind::Expand,
            records.get(&spec.id),
            Vec::new(),
            Some(format!("expand not lowered from {}", spec.source)),
            execution,
        ),
    }
}

fn push_leaf_execution(
    spec: &LeafSpec,
    records: &HashMap<String, RuntimeTaskRecord>,
    execution: &mut IrWorkflowExecution,
) {
    let record = records.get(&spec.id);
    let status = record
        .map(|record| record.status)
        .unwrap_or(IrWorkflowRunStatus::Pending);
    mark_ir_status(execution, status);
    execution.leaf_results.push(LeafResult {
        leaf_id: spec.id.clone(),
        task_id: record
            .map(|record| record.agent_id.clone())
            .unwrap_or_else(|| spec.id.clone()),
        profile: spec.profile.clone(),
        status,
        usage: WorkflowUsage::default(),
        memo_usage: WorkflowMemoUsage::default(),
        output: record.and_then(|record| record.output.clone()),
        artifacts: Vec::new(),
    });
}

fn push_branch_execution(
    spec: &BranchSpec,
    records: &HashMap<String, RuntimeTaskRecord>,
    execution: &mut IrWorkflowExecution,
) {
    let before = execution.leaf_results.len();
    for child in &spec.children {
        push_execution_node(child, records, execution);
    }
    let status = aggregate_ir_status(
        execution.leaf_results[before..]
            .iter()
            .map(|result| result.status),
    );
    mark_ir_status(execution, status);
    execution.branch_results.push(BranchResult {
        branch_id: spec.id.clone(),
        task_id: spec.id.clone(),
        status,
        usage: WorkflowUsage::default(),
        memo_usage: WorkflowMemoUsage::default(),
        artifacts: Vec::new(),
        notes: Some("production driver branch receipt from child task outcomes".to_string()),
    });
    execution.control_node_results.push(ControlNodeResult {
        node_id: spec.id.clone(),
        kind: ControlNodeKind::BranchSet,
        status,
        selected_children: spec.children.iter().map(declarative_node_id).collect(),
        summary: Some("branch set lowered into production child tasks".to_string()),
    });
}

fn push_sequence_execution(
    spec: &SequenceSpec,
    records: &HashMap<String, RuntimeTaskRecord>,
    execution: &mut IrWorkflowExecution,
) {
    let before_leaf = execution.leaf_results.len();
    let before_control = execution.control_node_results.len();
    for child in &spec.children {
        push_execution_node(child, records, execution);
    }
    let status = aggregate_ir_status(
        execution.leaf_results[before_leaf..]
            .iter()
            .map(|result| result.status)
            .chain(
                execution.control_node_results[before_control..]
                    .iter()
                    .map(|result| result.status),
            ),
    );
    mark_ir_status(execution, status);
    execution.control_node_results.push(ControlNodeResult {
        node_id: spec.id.clone(),
        kind: ControlNodeKind::Sequence,
        status,
        selected_children: spec.children.iter().map(declarative_node_id).collect(),
        summary: Some("sequence lowered in declaration order".to_string()),
    });
}

fn push_control_execution(
    node_id: &str,
    kind: ControlNodeKind,
    record: Option<&RuntimeTaskRecord>,
    selected_children: Vec<String>,
    fallback_summary: Option<String>,
    execution: &mut IrWorkflowExecution,
) {
    let status = record
        .map(|record| record.status)
        .unwrap_or(IrWorkflowRunStatus::Pending);
    mark_ir_status(execution, status);
    execution.control_node_results.push(ControlNodeResult {
        node_id: node_id.to_string(),
        kind,
        status,
        selected_children,
        summary: record
            .and_then(|record| record.output.clone())
            .or(fallback_summary),
    });
}

fn aggregate_ir_status(
    statuses: impl IntoIterator<Item = IrWorkflowRunStatus>,
) -> IrWorkflowRunStatus {
    let mut saw_pending = false;
    let mut saw_running = false;
    for status in statuses {
        match status {
            IrWorkflowRunStatus::BudgetExceeded => return IrWorkflowRunStatus::BudgetExceeded,
            IrWorkflowRunStatus::Cancelled => return IrWorkflowRunStatus::Cancelled,
            IrWorkflowRunStatus::Failed | IrWorkflowRunStatus::ReplayDiverged => {
                return IrWorkflowRunStatus::Failed;
            }
            IrWorkflowRunStatus::Running => saw_running = true,
            IrWorkflowRunStatus::Pending => saw_pending = true,
            IrWorkflowRunStatus::Succeeded => {}
        }
    }
    if saw_running {
        IrWorkflowRunStatus::Running
    } else if saw_pending {
        IrWorkflowRunStatus::Pending
    } else {
        IrWorkflowRunStatus::Succeeded
    }
}

fn mark_ir_status(execution: &mut IrWorkflowExecution, status: IrWorkflowRunStatus) {
    match status {
        IrWorkflowRunStatus::Failed | IrWorkflowRunStatus::ReplayDiverged => {
            execution.mark_failed()
        }
        IrWorkflowRunStatus::Cancelled => execution.mark_cancelled(),
        IrWorkflowRunStatus::BudgetExceeded => execution.mark_budget_exceeded(),
        IrWorkflowRunStatus::Running => {
            if execution.status == IrWorkflowRunStatus::Succeeded {
                execution.status = IrWorkflowRunStatus::Running;
            }
        }
        IrWorkflowRunStatus::Pending => {
            if execution.status == IrWorkflowRunStatus::Succeeded {
                execution.status = IrWorkflowRunStatus::Pending;
            }
        }
        IrWorkflowRunStatus::Succeeded => {}
    }
}

fn declarative_node_id(node: &WorkflowNode) -> String {
    match node {
        WorkflowNode::BranchSet(spec) => spec.id.clone(),
        WorkflowNode::Leaf(spec) => spec.id.clone(),
        WorkflowNode::Sequence(spec) => spec.id.clone(),
        WorkflowNode::Reduce(spec) => spec.id.clone(),
        WorkflowNode::TeacherReview(spec) => spec.id.clone(),
        WorkflowNode::LoopUntil(spec) => spec.id.clone(),
        WorkflowNode::Cond(spec) => spec.id.clone(),
        WorkflowNode::Expand(spec) => spec.id.clone(),
    }
}

fn spawn_completion_pump(
    driver: Arc<SubAgentWorkflowDriver>,
    mut rx: mpsc::UnboundedReceiver<SubAgentCompletion>,
) {
    spawn_supervised(
        "workflow-completion-pump",
        std::panic::Location::caller(),
        async move {
            while let Some(completion) = rx.recv().await {
                let agent_id = completion.agent_id.clone();
                let task_completion =
                    completion_from_manager(driver.manager.clone(), &agent_id, completion.payload)
                        .await;
                driver.deliver_completion(agent_id, task_completion);
            }
        },
    );
}

async fn completion_from_manager(
    manager: SharedSubAgentManager,
    agent_id: &str,
    fallback_payload: String,
) -> TaskCompletion {
    for _ in 0..50 {
        let snapshot = {
            let manager = manager.read().await;
            manager.get_result(agent_id).ok()
        };
        if let Some(snapshot) = snapshot
            && snapshot.status != SubAgentStatus::Running
        {
            return match snapshot.status {
                SubAgentStatus::Completed => TaskCompletion::Completed {
                    text: snapshot.result.unwrap_or(fallback_payload),
                },
                SubAgentStatus::Failed(message) => TaskCompletion::Failed { message },
                SubAgentStatus::Interrupted(message) => TaskCompletion::Failed { message },
                SubAgentStatus::Cancelled => TaskCompletion::Cancelled,
                SubAgentStatus::BudgetExhausted => TaskCompletion::BudgetExhausted {
                    message: "sub-agent budget exhausted".to_string(),
                },
                SubAgentStatus::Running => unreachable!("guarded above"),
            };
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    TaskCompletion::Completed {
        text: fallback_payload,
    }
}

fn cancel_child_agents(manager: SharedSubAgentManager, ids: Vec<String>) {
    if ids.is_empty() {
        return;
    }
    if let Ok(mut manager_guard) = manager.try_write() {
        for id in ids {
            let _ = manager_guard.cancel_agent(&id);
        }
        return;
    }
    if tokio::runtime::Handle::try_current().is_ok() {
        spawn_supervised(
            "workflow-cancel-children",
            std::panic::Location::caller(),
            async move {
                let mut manager_guard = manager.write().await;
                for id in ids {
                    let _ = manager_guard.cancel_agent(&id);
                }
            },
        );
    }
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, ToolError> {
    mutex
        .lock()
        .map_err(|_| ToolError::execution_failed("workflow state lock poisoned"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::DeepSeekClient;
    use crate::tools::ToolRegistryBuilder;
    use crate::tools::subagent::{SubAgentRuntime, new_shared_subagent_manager};
    use axum::{Json, Router, routing::post};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn workflow_action_defaults_to_start() {
        assert_eq!(
            parse_workflow_action(&json!({})).unwrap(),
            WorkflowAction::Start
        );
        assert_eq!(
            parse_workflow_action(&json!({"action": "run"})).unwrap(),
            WorkflowAction::Run
        );
    }

    #[test]
    fn inline_script_and_source_path_are_mutually_exclusive() {
        let ctx = ToolContext::new(".");
        let err = workflow_source(
            &json!({
                "script": "return 1;",
                "source_path": "workflow.js"
            }),
            &ctx,
        )
        .unwrap_err();
        assert!(err.to_string().contains("either script or source_path"));
    }

    #[test]
    fn source_path_must_stay_inside_workspace_without_trust_mode() {
        let workspace = tempfile::tempdir().expect("workspace tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        let outside_path = outside.path().join("outside.workflow.js");
        std::fs::write(&outside_path, "return 1;").expect("outside workflow source");
        let ctx = ToolContext::new(workspace.path().to_path_buf());

        let err = workflow_source(
            &json!({
                "source_path": outside_path
            }),
            &ctx,
        )
        .expect_err("outside source_path should be denied");

        assert!(
            err.to_string().contains("must stay inside the workspace"),
            "{err}"
        );
    }

    #[test]
    fn subagent_tool_surface_registers_workflow_and_agent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let manager = new_shared_subagent_manager(tmp.path().to_path_buf(), 2);
        let runtime = SubAgentRuntime::new(
            stub_client(),
            "deepseek-v4-flash".to_string(),
            ctx.clone(),
            true,
            None,
            manager.clone(),
        );
        let registry = ToolRegistryBuilder::new()
            .with_subagent_tools(manager, runtime)
            .build(ctx);

        assert!(registry.contains("workflow"));
        assert!(registry.contains("agent"));
        assert!(
            registry
                .to_api_tools()
                .iter()
                .any(|tool| tool.name == "workflow")
        );
    }

    #[tokio::test]
    async fn workflow_run_dispatches_task_through_subagent_manager() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let manager = new_shared_subagent_manager(tmp.path().to_path_buf(), 2);
        let (client, calls) = fake_chat_client("child done").await;
        let runtime = SubAgentRuntime::new(
            client,
            "deepseek-v4-flash".to_string(),
            ctx.clone(),
            true,
            None,
            manager.clone(),
        );
        let tool = WorkflowTool::new(manager.clone(), runtime);

        let result = tool
            .execute(
                json!({
                    "action": "run",
                    "script": "const out = await task({ description: 'say done', type: 'explore', allowedTools: [] }); return { out };"
                }),
                &ctx,
            )
            .await
            .expect("workflow run should complete");
        let payload: Value = serde_json::from_str(&result.content).expect("json result");

        assert_eq!(payload["status"], "completed");
        assert_eq!(payload["result"]["out"], "child done");
        assert_eq!(payload["child_ids"].as_array().unwrap().len(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let child_id = payload["child_ids"][0].as_str().unwrap();
        let child = manager
            .read()
            .await
            .get_result(child_id)
            .expect("child result");
        assert_eq!(child.status, SubAgentStatus::Completed);
        assert_eq!(child.result.as_deref(), Some("child done"));
    }

    #[tokio::test]
    async fn declarative_parallel_spawn_failure_fails_before_reduce() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let manager = new_shared_subagent_manager(tmp.path().to_path_buf(), 2);
        let (client, calls) = fake_chat_client("should not be called").await;
        let runtime = SubAgentRuntime::new(
            client,
            "deepseek-v4-flash".to_string(),
            ctx.clone(),
            true,
            None,
            manager,
        );
        let tool = WorkflowTool::new(runtime.manager.clone(), runtime);

        let result = tool
            .execute(
                json!({
                    "action": "run",
                    "script": r#"export default workflow({
                        "goal": "fail fast",
                        "nodes": [
                            {
                                "branch": {
                                    "id": "parallel",
                                    "parallel": true,
                                    "children": [
                                        {
                                            "agent": {
                                                "id": "bad-profile",
                                                "prompt": "This child should be rejected before model execution.",
                                                "profile": "missing-profile"
                                            }
                                        }
                                    ]
                                }
                            },
                            {
                                "reduce": {
                                    "id": "summary",
                                    "inputs": ["bad-profile"],
                                    "prompt": "This reduce must not run."
                                }
                            }
                        ]
                    });"#
                }),
                &ctx,
            )
            .await
            .expect("failed workflow still returns run record");
        let payload: Value = serde_json::from_str(&result.content).expect("json result");

        assert_eq!(payload["status"], "failed");
        assert!(
            payload["error"]
                .as_str()
                .unwrap()
                .contains("Unknown profile 'missing-profile'"),
            "{}",
            payload["error"]
        );
        assert!(payload["result"].is_null());
        assert_eq!(payload["execution"]["status"], "failed");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn declarative_dependency_results_are_forwarded_to_downstream_prompt() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let manager = new_shared_subagent_manager(tmp.path().to_path_buf(), 2);
        let (client, calls, bodies) = fake_chat_client_capturing("upstream-output").await;
        let runtime = SubAgentRuntime::new(
            client,
            "deepseek-v4-flash".to_string(),
            ctx.clone(),
            true,
            None,
            manager,
        );
        let tool = WorkflowTool::new(runtime.manager.clone(), runtime);

        let result = tool
            .execute(
                json!({
                    "action": "run",
                    "script": r#"export default workflow({
                        "goal": "dependency forwarding",
                        "nodes": [
                            {
                                "agent": {
                                    "id": "first",
                                    "prompt": "Produce the upstream finding.",
                                    "agent_type": "review"
                                }
                            },
                            {
                                "agent": {
                                    "id": "second",
                                    "prompt": "Use the upstream finding.",
                                    "agent_type": "review",
                                    "depends_on_results": ["first"]
                                }
                            }
                        ]
                    });"#
                }),
                &ctx,
            )
            .await
            .expect("dependency workflow should complete");
        let payload: Value = serde_json::from_str(&result.content).expect("json result");

        assert_eq!(payload["status"], "completed");
        assert_eq!(payload["execution"]["status"], "succeeded");
        assert_eq!(
            payload["execution"]["leaf_results"][0]["output"],
            "upstream-output"
        );
        assert_eq!(
            payload["execution"]["leaf_results"][1]["output"],
            "upstream-output"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let bodies = bodies.lock().expect("captured bodies");
        let second_body = bodies.get(1).expect("second provider call").to_string();
        assert!(second_body.contains("--- first ---"), "{second_body}");
        assert!(second_body.contains("upstream-output"), "{second_body}");
    }

    #[tokio::test]
    async fn declarative_issue_audit_fixture_runs_through_subagent_driver() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflow_dir = tmp.path().join("workflows");
        std::fs::create_dir_all(&workflow_dir).expect("workflow dir");
        let fixture = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../workflows/issue_audit.workflow.js"),
        )
        .expect("issue audit fixture");
        std::fs::write(workflow_dir.join("issue_audit.workflow.js"), fixture)
            .expect("write fixture into workspace");

        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let manager = new_shared_subagent_manager(tmp.path().to_path_buf(), 4);
        let (client, calls) = fake_chat_client("audited").await;
        let runtime = SubAgentRuntime::new(
            client,
            "deepseek-v4-flash".to_string(),
            ctx.clone(),
            true,
            None,
            manager,
        );
        let tool = WorkflowTool::new(runtime.manager.clone(), runtime);

        let result = tool
            .execute(
                json!({
                    "action": "run",
                    "source_path": "workflows/issue_audit.workflow.js"
                }),
                &ctx,
            )
            .await
            .expect("declarative workflow should complete");
        let payload: Value = serde_json::from_str(&result.content).expect("json result");

        assert_eq!(payload["status"], "completed");
        assert_eq!(payload["result"]["code-audit"], "audited");
        assert_eq!(payload["result"]["test-audit"], "audited");
        assert_eq!(payload["result"]["docs-audit"], "audited");
        assert_eq!(payload["result"]["synthesize-release-risk"], "audited");
        assert_eq!(payload["execution"]["status"], "succeeded");
        assert_eq!(
            payload["execution"]["leaf_results"]
                .as_array()
                .expect("leaf results")
                .len(),
            3
        );
        assert_eq!(
            payload["execution"]["branch_results"][0]["branch_id"],
            "parallel-audit"
        );
        assert!(
            payload["execution"]["control_node_results"]
                .as_array()
                .expect("control results")
                .iter()
                .any(|result| result["node_id"] == "synthesize-release-risk"
                    && result["kind"] == "reduce"
                    && result["status"] == "succeeded")
        );
        assert_eq!(payload["child_ids"].as_array().unwrap().len(), 4);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert!(
            payload["progress"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message == "phase: parallel-audit")
        );
    }

    fn stub_client() -> DeepSeekClient {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let config = crate::config::Config {
            api_key: Some("test-key".to_string()),
            ..crate::config::Config::default()
        };
        DeepSeekClient::new(&config).expect("stub client should construct")
    }

    async fn fake_chat_client(response_text: &str) -> (DeepSeekClient, Arc<AtomicUsize>) {
        let (client, calls, _) = fake_chat_client_capturing(response_text).await;
        (client, calls)
    }

    async fn fake_chat_client_capturing(
        response_text: &str,
    ) -> (DeepSeekClient, Arc<AtomicUsize>, Arc<Mutex<Vec<Value>>>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let response_text = response_text.to_string();
        let app = Router::new().route(
            "/{*path}",
            post({
                let calls = Arc::clone(&calls);
                let bodies = Arc::clone(&bodies);
                move |Json(body): Json<Value>| {
                    let calls = Arc::clone(&calls);
                    let bodies = Arc::clone(&bodies);
                    let response_text = response_text.clone();
                    async move {
                        bodies.lock().expect("capture body").push(body);
                        let attempt = calls.fetch_add(1, Ordering::SeqCst) + 1;
                        Json(json!({
                            "id": format!("chatcmpl-workflow-test-{attempt}"),
                            "model": "deepseek-v4-flash",
                            "choices": [{
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": response_text
                                },
                                "finish_reason": "stop"
                            }],
                            "usage": {
                                "prompt_tokens": 1,
                                "completion_tokens": 1,
                                "total_tokens": 2
                            }
                        }))
                    }
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake chat server");
        let addr = listener.local_addr().expect("fake chat server addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let config = crate::config::Config {
            api_key: Some("test-key".to_string()),
            base_url: Some(format!("http://{addr}/v1")),
            ..crate::config::Config::default()
        };
        (
            DeepSeekClient::new(&config).expect("fake chat client"),
            calls,
            bodies,
        )
    }
}
