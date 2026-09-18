// Read a `mynd.*` localStorage value, transparently adopting a value written
// under the pre-rename `hivemind.*` key (then clearing the old one) so per-
// browser dashboard state survives the rename. Returns null when absent, and
// is safe when storage is unavailable (private windows, blocked site data).
export function readStored(key) {
  try {
    const current = localStorage.getItem(key)
    if (current !== null) return current
    const legacyKey = key.replace(/^mynd\./, 'hivemind.')
    if (legacyKey === key) return null
    const legacy = localStorage.getItem(legacyKey)
    if (legacy === null) return null
    localStorage.setItem(key, legacy)
    localStorage.removeItem(legacyKey)
    return legacy
  } catch {
    return null
  }
}
