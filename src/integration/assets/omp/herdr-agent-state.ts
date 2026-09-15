// installed by herdr
// managed by herdr; reinstalling or updating the integration overwrites this file.
// add custom hooks/plugins beside this file instead of editing it.
// HERDR_INTEGRATION_ID=omp
// HERDR_INTEGRATION_VERSION=9
// LOCAL PATCH (cubix): daemon-aware pane identity. Under `omp --daemon`
// this extension runs in the shared daemon process, whose env belongs to
// whichever pane spawned the daemon first. Pane identity is resolved
// per-session from `ctx.clientEnv` (OMP forwards the attached client's
// terminal-identity env), falling back to `process.env` in direct mode.
// @ts-nocheck

import net from "node:net";

const source = "herdr:omp";

type AgentState = "working" | "blocked" | "idle";

type QueuedState = {
  state: AgentState;
  message?: string;
  seq: number;
};

type HerdrIdentity = {
  socketEndpoint: string;
  paneId: string;
  idleDebounceMs: number;
  retryGraceMs: number;
};

const retryableErrorPattern =
  /overloaded|provider.?returned.?error|rate.?limit|too many requests|429|500|502|503|504|service.?unavailable|server.?error|internal.?error|network.?error|connection.?error|connection.?refused|connection.?lost|websocket.?closed|websocket.?error|other side closed|fetch failed|upstream.?connect|reset before headers|socket hang up|ended without|http2 request did not get a response|timed? out|timeout|terminated|retry delay/i;

function parseDuration(raw: string | undefined, fallback: number): number {
  if (!raw) {
    return fallback;
  }
  const parsed = Number.parseInt(raw, 10);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return fallback;
  }
  return parsed;
}

function socketEndpointFor(socketPath: string): string {
  return process.platform === "win32" ? `\\\\.\\pipe\\${socketPath}` : socketPath;
}

/**
 * Resolve the herdr pane identity for THIS session's attached client.
 * Hosted sessions get the client terminal env via ctx.clientEnv; direct mode
 * falls back to this process's env (the historical behavior).
 */
function resolveIdentity(ctx: any): HerdrIdentity | undefined {
  const env: Record<string, string | undefined> = ctx?.clientEnv ?? process.env;
  if (env.HERDR_ENV !== "1") {
    return undefined;
  }
  const socketPath = env.HERDR_SOCKET_PATH;
  const paneId = env.HERDR_PANE_ID;
  if (!socketPath || !paneId) {
    return undefined;
  }
  return {
    socketEndpoint: socketEndpointFor(socketPath),
    paneId,
    idleDebounceMs: parseDuration(env.HERDR_OMP_IDLE_DEBOUNCE_MS, 250),
    retryGraceMs: parseDuration(env.HERDR_OMP_RETRY_GRACE_MS, 2500),
  };
}

function sendRequestAttempt(socketEndpoint: string, request: unknown, timeoutMs: number): Promise<boolean> {
  return new Promise((resolve) => {
    let done = false;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    const finish = (delivered: boolean) => {
      if (done) return;
      done = true;
      if (timeout) {
        clearTimeout(timeout);
      }
      socket.destroy();
      resolve(delivered);
    };

    const socket = net.createConnection(socketEndpoint);
    socket.on("error", () => finish(false));
    socket.on("connect", () => socket.write(`${JSON.stringify(request)}\n`));
    socket.on("data", () => finish(true));
    socket.on("end", () => finish(false));
    timeout = setTimeout(() => finish(false), timeoutMs);
    timeout.unref?.();
  });
}

function lastAssistantMessage(messages: unknown[]): any | undefined {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i] as any;
    if (message?.role === "assistant") {
      return message;
    }
  }
  return undefined;
}

function retryableErrorMessage(event: any): string | undefined {
  const messages = Array.isArray(event?.messages) ? event.messages : [];
  const assistant = lastAssistantMessage(messages);
  if (assistant?.stopReason !== "error") {
    return undefined;
  }

  const errorMessage = String(assistant.errorMessage ?? "");
  if (!retryableErrorPattern.test(errorMessage)) {
    return undefined;
  }
  return errorMessage || "retryable provider error";
}

function askBlockedMessage(args: any): string {
  const questions = Array.isArray(args?.questions) ? args.questions : [];
  const firstQuestion = questions.find((question: any) => typeof question?.question === "string");
  if (firstQuestion?.question) {
    return firstQuestion.question;
  }
  return "waiting for user input";
}

export default function (pi) {
  // Per-session state: this factory runs once per OMP session (also inside the
  // shared daemon), so pane identity and the report pipeline live here rather
  // than at module scope where the first pane would win for every session.
  let identity: HerdrIdentity | undefined;
  let requestQueue = Promise.resolve();
  let reportSeq = Date.now() * 1000;
  let currentAgentSessionId: string | undefined;
  let currentAgentSessionPath: string | undefined;

  let agentActive = false;
  let retryHoldActive = false;
  let blockedCount = 0;
  let blockedMessage: string | undefined;
  let lastState: AgentState | undefined;
  let lastMessage: string | undefined;
  let idleTimer: ReturnType<typeof setTimeout> | undefined;
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  let rootSession = false;
  let sendInFlight = false;
  let queuedState: QueuedState | undefined;
  let stateQueueWaiters: Array<() => void> = [];

  function refreshIdentity(ctx: any): boolean {
    // The attached client's env is authoritative: a reattach from a
    // non-herdr terminal must stop reporting to the previous pane, so a
    // failed resolve clears the identity instead of keeping the stale one.
    const previous = identity;
    identity = resolveIdentity(ctx);
    if (
      previous &&
      (identity === undefined ||
        previous.paneId !== identity.paneId ||
        previous.socketEndpoint !== identity.socketEndpoint)
    ) {
      // The session moved to another terminal: drop state queued for the old
      // pane and free it so it does not stay stuck on the last reported
      // state. Clearing lastState guarantees the caller's next publishState
      // (which runs after updateSessionRef) reaches the new pane even when
      // the agent state itself did not change.
      queuedState = undefined;
      lastState = undefined;
      lastMessage = undefined;
      void releaseAgentFor(previous);
    }
    return identity !== undefined;
  }

  function nextReportSeq(): number {
    reportSeq += 1;
    return reportSeq;
  }

  async function sendRequestNow(socketEndpoint: string, request: unknown): Promise<void> {
    if (await sendRequestAttempt(socketEndpoint, request, 500)) {
      return;
    }
    await sendRequestAttempt(socketEndpoint, request, 1500);
  }

  // Route is snapshotted at enqueue time: an identity switch (reattach to a
  // different pane) must not redirect queued/retried sends to the new socket.
  function sendRequest(target: HerdrIdentity, request: unknown): Promise<void> {
    const socketEndpoint = target.socketEndpoint;
    requestQueue = requestQueue.then(
      () => sendRequestNow(socketEndpoint, request),
      () => sendRequestNow(socketEndpoint, request),
    );
    return requestQueue;
  }

  function updateSessionRef(ctx: any): void {
    try {
      const file = ctx?.sessionManager?.getSessionFile?.();
      currentAgentSessionPath =
        typeof file === "string" && file.startsWith("/") ? file : undefined;
    } catch {
      currentAgentSessionPath = undefined;
    }

    try {
      const id = ctx?.sessionManager?.getSessionId?.();
      currentAgentSessionId = typeof id === "string" && id.length > 0 ? id : undefined;
    } catch {
      currentAgentSessionId = undefined;
    }
  }

  function withSessionRef(params: Record<string, unknown>): Record<string, unknown> {
    if (currentAgentSessionPath) {
      return { ...params, agent_session_path: currentAgentSessionPath };
    }
    if (currentAgentSessionId) {
      return { ...params, agent_session_id: currentAgentSessionId };
    }
    return params;
  }

  function currentSessionRef(): Record<string, unknown> | undefined {
    if (currentAgentSessionPath) {
      return { agent_session_path: currentAgentSessionPath };
    }
    if (currentAgentSessionId) {
      return { agent_session_id: currentAgentSessionId };
    }
    return undefined;
  }

  function reportSession(sessionStartSource = "startup"): Promise<void> {
    const target = identity;
    const sessionRef = currentSessionRef();
    if (!sessionRef || !target) {
      return Promise.resolve();
    }

    return sendRequest(target, {
      id: `${source}:session:${Date.now()}:${Math.random().toString(36).slice(2)}`,
      method: "pane.report_agent_session",
      params: {
        pane_id: target.paneId,
        source,
        agent: "omp",
        seq: nextReportSeq(),
        session_start_source: sessionStartSource,
        ...sessionRef,
      },
    });
  }

  function sendState(state: AgentState, message?: string, seq = nextReportSeq()): Promise<void> {
    const target = identity;
    if (!target) {
      return Promise.resolve();
    }
    return sendRequest(target, {
      id: `${source}:${Date.now()}:${Math.random().toString(36).slice(2)}`,
      method: "pane.report_agent",
      params: withSessionRef({
        pane_id: target.paneId,
        source,
        agent: "omp",
        state,
        message,
        seq,
      }),
    });
  }

  function releaseAgentFor(target: HerdrIdentity): Promise<void> {
    // Only used for identity switches (reattach to a different pane). Process
    // exit stays process-owned: herdr releases lifecycle authority itself when
    // the agent process dies, so session_shutdown deliberately does not send
    // this.
    return sendRequest(target, {
      id: `${source}:release:${Date.now()}:${Math.random().toString(36).slice(2)}`,
      method: "pane.release_agent",
      params: {
        pane_id: target.paneId,
        source,
        agent: "omp",
        seq: nextReportSeq(),
      },
    });
  }

  function queueState(state: AgentState, message?: string): void {
    queuedState = { state, message, seq: nextReportSeq() };
    if (!sendInFlight) {
      void drainStateQueue();
    }
  }

  async function drainStateQueue(): Promise<void> {
    if (sendInFlight) {
      return;
    }

    sendInFlight = true;
    try {
      while (queuedState) {
        const next = queuedState;
        queuedState = undefined;
        await sendState(next.state, next.message, next.seq);
      }
    } finally {
      sendInFlight = false;
      if (queuedState) {
        void drainStateQueue();
      } else {
        const waiters = stateQueueWaiters;
        stateQueueWaiters = [];
        for (const resolve of waiters) {
          resolve();
        }
      }
    }
  }

  function waitForStateQueue(): Promise<void> {
    if (!sendInFlight && !queuedState) {
      return Promise.resolve();
    }
    const { promise, resolve } = Promise.withResolvers<void>();
    stateQueueWaiters.push(resolve);
    return promise;
  }

  function clearTimer(timer: ReturnType<typeof setTimeout> | undefined) {
    if (timer) {
      clearTimeout(timer);
    }
  }

  function clearPendingTimers() {
    clearTimer(idleTimer);
    clearTimer(retryTimer);
    idleTimer = undefined;
    retryTimer = undefined;
  }

  function clearFailureState() {
    retryHoldActive = false;
  }

  function desiredState() {
    if (blockedCount > 0) {
      return { state: "blocked" as const, message: blockedMessage };
    }
    if (agentActive || retryHoldActive) {
      return { state: "working" as const, message: undefined };
    }
    return { state: "idle" as const, message: undefined };
  }

  function publishState(force = false) {
    const next = desiredState();
    if (!force && next.state === lastState && next.message === lastMessage) {
      return;
    }
    lastState = next.state;
    lastMessage = next.message;
    queueState(next.state, next.message);
  }

  function scheduleIdle() {
    clearPendingTimers();
    clearFailureState();
    idleTimer = setTimeout(() => {
      idleTimer = undefined;
      publishState();
    }, identity?.idleDebounceMs ?? 250);
    idleTimer.unref?.();
  }

  function scheduleStartupReconciliation(ctx: { isIdle?: () => boolean }) {
    clearTimer(idleTimer);
    let checksRemaining = 2;
    const reconcile = () => {
      idleTimer = undefined;
      if (!rootSession || !agentActive) {
        return;
      }
      if (ctx?.isIdle?.() === true) {
        agentActive = false;
        publishState();
        return;
      }
      checksRemaining -= 1;
      if (checksRemaining === 0) {
        return;
      }
      idleTimer = setTimeout(reconcile, identity?.retryGraceMs ?? 2500);
      idleTimer.unref?.();
    };
    idleTimer = setTimeout(reconcile, identity?.idleDebounceMs ?? 250);
    idleTimer.unref?.();
  }
  function holdForRetry() {
    clearPendingTimers();
    retryHoldActive = true;
    publishState();

    retryTimer = setTimeout(() => {
      retryTimer = undefined;
      retryHoldActive = false;
      publishState();
    }, identity?.retryGraceMs ?? 2500);
    retryTimer.unref?.();
  }

  function activateRootSession(ctx: any, sessionStartSource = "startup"): boolean {
    if (ctx?.hasUI !== true) {
      return false;
    }
    if (!refreshIdentity(ctx)) {
      return false;
    }
    rootSession = true;
    updateSessionRef(ctx);
    void reportSession(sessionStartSource);
    return true;
  }

  function resetSessionState() {
    clearPendingTimers();
    clearFailureState();
    agentActive = false;
    blockedCount = 0;
    blockedMessage = undefined;
  }

  function activateBlocked(message: string | undefined) {
    clearPendingTimers();
    blockedCount += 1;
    blockedMessage = message;
    publishState();
  }

  function deactivateBlocked() {
    blockedCount = Math.max(0, blockedCount - 1);
    if (blockedCount === 0) {
      blockedMessage = undefined;
    }
    publishState();
  }

  pi.events.on("herdr:blocked", (data) => {
    if (!rootSession) {
      return;
    }
    if (!data?.active) {
      deactivateBlocked();
      return;
    }

    activateBlocked(data.label);
  });

  pi.on("session_start", (_event, ctx) => {
    if (!activateRootSession(ctx)) {
      return;
    }
    // A reload can replace this extension mid-run without emitting another agent_start.
    agentActive = ctx?.isIdle?.() === false;
    publishState(true);
    if (agentActive) {
      scheduleStartupReconciliation(ctx);
    }
  });

  pi.on("session_switch", (event, ctx) => {
    if (!activateRootSession(ctx, event?.reason || "resume")) {
      return;
    }
    resetSessionState();
    publishState(true);
  });

  pi.on("agent_start", (_event, ctx) => {
    if (!rootSession && !activateRootSession(ctx)) {
      return;
    }
    refreshIdentity(ctx);
    updateSessionRef(ctx);
    void reportSession();
    clearPendingTimers();
    clearFailureState();
    agentActive = true;
    publishState();
  });

  pi.on("tool_approval_requested", (event, ctx) => {
    if (!rootSession && !activateRootSession(ctx)) {
      return;
    }
    const label = event?.reason || `${event?.toolName || "Tool"} approval`;
    activateBlocked(label);
  });

  pi.on("tool_approval_resolved", (_event, ctx) => {
    if (!rootSession && !activateRootSession(ctx)) {
      return;
    }
    deactivateBlocked();
  });

  pi.on("tool_execution_start", (event, ctx) => {
    if (event?.toolName !== "ask") {
      return;
    }
    if (!rootSession && !activateRootSession(ctx)) {
      return;
    }
    activateBlocked(askBlockedMessage(event.args));
  });

  pi.on("tool_execution_end", (event, ctx) => {
    if (event?.toolName !== "ask") {
      return;
    }
    if (!rootSession && !activateRootSession(ctx)) {
      return;
    }
    deactivateBlocked();
  });

  pi.on("agent_end", (event) => {
    if (!rootSession) {
      return;
    }
    if (!agentActive) {
      // OMP can emit duplicate/late end events while auto-retry is already
      // holding the pane in Working. Do not let an unqualified duplicate end
      // cancel the retry hold and publish a false Idle.
      return;
    }

    agentActive = false;

    const retryableMessage = retryableErrorMessage(event);
    if (retryableMessage) {
      holdForRetry();
      return;
    }

    scheduleIdle();
  });

  pi.on("session_shutdown", async (_event, ctx) => {
    if (!rootSession) {
      return;
    }
    const flushIdle =
      idleTimer !== undefined && (!agentActive || ctx?.isIdle?.() === true);
    clearPendingTimers();
    if (flushIdle) {
      agentActive = false;
      publishState();
      await waitForStateQueue();
    }
  });
}
