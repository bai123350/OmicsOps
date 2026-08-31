const status = document.querySelector("#status");
const session = new URL(location.href).searchParams.get("session");

if (session !== "shared" && session !== "workspace") {
  status.textContent = "Invalid OmicsOps browser session.";
} else {
  chrome.runtime.sendMessage({ type: "omicsops.select_session", session }).then((reply) => {
    status.textContent = reply?.ok
      ? `OmicsOps ${session} session selected. You may return to the app.`
      : `Could not connect the OmicsOps ${session} session: ${reply?.error || "unknown error"}`;
  }).catch((error) => {
    status.textContent = `Could not select the OmicsOps ${session} session: ${String(error?.message || error)}`;
  });
}
