import { request } from './client.js'

export const listMemories = (limit) => request('GET', '/api/v1/memories' + (limit ? `?limit=${limit}` : ''))
export const createMemory = (body) => request('POST', '/api/v1/memories', body)
export const patchMemory = (id, body) => request('PATCH', `/api/v1/memories/${id}`, body)
export const addMemoryTags = (id, tags) => request('POST', `/api/v1/memories/${id}/tags/add`, { tags })
export const removeMemoryTags = (id, tags) => request('POST', `/api/v1/memories/${id}/tags/remove`, { tags })
export const searchMemories = (q) => request('GET', `/api/v1/search?q=${encodeURIComponent(q)}`)
export const deleteMemory = (id) => request('DELETE', `/api/v1/memories/${id}`)
export const deleteAllMemories = () => request('DELETE', '/api/v1/memories/all')
export const exportMemories = () => request('GET', '/api/v1/export')
export const importMemories = (body) => request('POST', '/api/v1/import', body)
export const getStatus = () => request('GET', '/api/v1/status')
export const countTokens = (body) => request('POST', '/api/v1/memories/count-tokens', body)
export const getProjectTree = (name, layer = 'workspace') =>
  request('GET', `/api/v1/projects/${encodeURIComponent(name)}/tree?layer=${layer}`)
