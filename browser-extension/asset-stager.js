import { assertAllowedUrl, sanitizeUrlForReport } from "./url-policy.js";

const MAX_ASSETS_PER_REQUEST = 100;
const MAX_FILENAME_LENGTH = 240;
const MAX_RELATIVE_PATH_LENGTH = 1024;
const CHECKSUM = /^[a-f0-9]{64}$/i;
const STAGED_ID = /^[a-zA-Z0-9_.-]{1,128}$/;

function generatedStageId() {
  if (!globalThis.crypto?.randomUUID) throw new Error("secure staged ID generation is unavailable");
  return `asset_${globalThis.crypto.randomUUID()}`;
}

function validateFilename(value) {
  if (value === undefined) return undefined;
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_FILENAME_LENGTH) {
    throw new Error("filename must be a non-empty string within the size limit");
  }
  if (/[\\/:*?"<>|\u0000-\u001f]/.test(value) || value === "." || value === "..") {
    throw new Error("filename contains a path separator or invalid character");
  }
  return value;
}

function validateRelativePath(value) {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_RELATIVE_PATH_LENGTH || value.trim() !== value) {
    throw new Error("relative_path must be a non-empty path within the size limit");
  }
  const normalized = value.replaceAll("\\", "/");
  if (normalized.startsWith("/") || /^[a-zA-Z]:[\\/]/.test(normalized) || normalized.split("/").some((part) => part === "" || part === "." || part === "..")) {
    throw new Error("relative_path must not escape the staging directory");
  }
  return normalized;
}

function validateStagedId(value) {
  if (typeof value !== "string" || !STAGED_ID.test(value) || value === "." || value === "..") {
    throw new Error("staged_id is invalid");
  }
  return value;
}

function validateBytes(value) {
  if (value === undefined) return undefined;
  if (!Number.isSafeInteger(value) || value < 0) throw new Error("bytes must be a non-negative safe integer");
  return value;
}

/**
 * Records download metadata only. The Host owns copying verified bytes from
 * Downloads/OmicsOps-Staging into a project; this object never stores bytes.
 */
export class AssetStager {
  constructor({ clock = Date.now, idFactory = generatedStageId } = {}) {
    this.clock = clock;
    this.idFactory = idFactory;
    this.records = new Map();
  }

  stage({ sessionId, turnId, tabId = undefined, assets }) {
    if (!Array.isArray(assets) || assets.length === 0 || assets.length > MAX_ASSETS_PER_REQUEST) {
      throw new Error("assets must be a non-empty array within the size limit");
    }
    const stagedAt = this.clock();
    const records = assets.map((asset) => {
      if (asset === null || typeof asset !== "object" || Array.isArray(asset)) throw new Error("each asset must be an object");
      if (asset.staged_id !== undefined) throw new Error("staged_id is extension-generated and must not be supplied");
      const stagedId = validateStagedId(this.idFactory());
      const relativePath = asset.relative_path === undefined
        ? validateFilename(asset.filename) || `${stagedId}.download`
        : validateRelativePath(asset.relative_path);
      const sourceUrl = asset.url ?? asset.source_url;
      if (sourceUrl !== undefined) assertAllowedUrl(sourceUrl);
      const bytes = validateBytes(asset.bytes ?? asset.size_bytes);
      if (asset.sha256 !== undefined && (typeof asset.sha256 !== "string" || !CHECKSUM.test(asset.sha256))) {
        throw new Error("sha256 must be a 64-character hexadecimal checksum");
      }
      const record = {
        staged_id: stagedId,
        relative_path: relativePath,
        session_id: sessionId,
        turn_id: turnId,
        ...(tabId === undefined ? {} : { tab_id: tabId }),
        ...(sourceUrl === undefined ? {} : { source_url: sanitizeUrlForReport(assertAllowedUrl(sourceUrl)) }),
        ...(asset.filename === undefined ? {} : { filename: validateFilename(asset.filename) }),
        ...(asset.mime_type === undefined ? {} : { mime_type: String(asset.mime_type).slice(0, 128) }),
        ...(bytes === undefined ? {} : { size_bytes: bytes }),
        ...(asset.sha256 === undefined ? {} : { sha256: asset.sha256.toLowerCase() }),
        ...(asset.title === undefined ? {} : { title: String(asset.title).slice(0, 512) }),
        staged_at: stagedAt,
        state: "staged_metadata",
      };
      this.records.set(stagedId, record);
      return { ...record };
    });
    return records;
  }

  list({ sessionId = undefined, turnId = undefined } = {}) {
    return [...this.records.values()]
      .filter((record) => sessionId === undefined || record.session_id === sessionId)
      .filter((record) => turnId === undefined || record.turn_id === turnId)
      .map((record) => ({ ...record }));
  }

  clear({ sessionId = undefined, turnId = undefined } = {}) {
    for (const [id, record] of this.records) {
      if ((sessionId === undefined || record.session_id === sessionId) && (turnId === undefined || record.turn_id === turnId)) this.records.delete(id);
    }
  }
}

export { validateRelativePath };
