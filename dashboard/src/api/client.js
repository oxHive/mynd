export const BASE = window.MYND_API || (import.meta.env.DEV ? '' : 'http://localhost:3456')

/**
 * @param {'GET'|'POST'|'PATCH'|'DELETE'} method
 * @param {string} path
 * @param {any} [body]
 */
export async function request(method, path, body) {
  const res = await fetch(BASE + path, {
    method,
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : {},
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) {
    const err = new Error(res.status + ' ' + res.statusText)
    err.status = res.status
    throw err
  }
  return res.status === 204 ? null : res.json()
}
