import { request } from './client.js'

export const getSyncSettings = () => request('GET', '/api/v1/settings/sync')
export const getConfigFile = () => request('GET', '/api/v1/settings/config')
export const saveConfigFile = (patch) => request('POST', '/api/v1/settings/config', patch)
export const restartServer = () => request('POST', '/api/v1/restart')
export const getTagSettings = () => request('GET', '/api/v1/settings/tags')
export const saveTagSettings = (body) => request('POST', '/api/v1/settings/tags', body)
export const getContentLimitSettings = () => request('GET', '/api/v1/settings/content-limits')
export const saveContentLimitSettings = (body) => request('POST', '/api/v1/settings/content-limits', body)
