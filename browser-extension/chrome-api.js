/** Small callback/Promise adapter for Chrome APIs and deterministic tests. */

export function getChromeApi() {
  return globalThis.chrome;
}

export function getChromeFunction(api, path) {
  if (!api || typeof path !== "string" || path.length === 0) return null;
  const parts = path.split(".");
  const name = parts.pop();
  const owner = parts.reduce((value, part) => value?.[part], api);
  return typeof owner?.[name] === "function" ? owner[name].bind(owner) : null;
}

export function callChrome(fn, args = []) {
  if (typeof fn !== "function") return Promise.reject(new Error("Chrome API is unavailable"));
  return new Promise((resolve, reject) => {
    let settled = false;
    const finish = (callback, value) => {
      if (settled) return;
      settled = true;
      callback(value);
    };
    const callback = (value) => {
      const runtimeError = globalThis.chrome?.runtime?.lastError;
      if (runtimeError) finish(reject, new Error(runtimeError.message || "Chrome API error"));
      else finish(resolve, value);
    };
    let returned;
    try {
      if (fn.length > args.length) returned = fn(...args, callback);
      else returned = fn(...args);
    } catch (error) {
      finish(reject, error);
      return;
    }
    if (returned && typeof returned.then === "function") {
      returned.then((value) => finish(resolve, value), (error) => finish(reject, error));
    } else if (fn.length <= args.length) {
      finish(resolve, returned);
    }
  });
}

export function chromeMethod(api, path) {
  if (!api || typeof path !== "string" || path.length === 0) return null;
  let owner = api;
  const parts = path.split(".");
  for (const part of parts) {
    owner = owner?.[part];
  }
  if (typeof owner !== "function") return null;
  return owner.bind(parts.slice(0, -1).reduce((value, part) => value?.[part], api));
}
