import { request } from './client.js'

export const getHiveStatus = () => request('GET', '/api/v1/hive/status')
export const setHiveEnabled = (enabled) => request('POST', '/api/v1/hive/enabled', { enabled })
export const issuePairingCode = () => request('POST', '/api/v1/hive/pairing-code')
export const revokeDevice = (deviceId) => request('POST', `/api/v1/hive/roster/${deviceId}/revoke`)
export const getTrustedNetworks = () => request('GET', '/api/v1/hive/trusted-networks')
export const addTrustedNetwork = (id, label) => request('POST', '/api/v1/hive/trusted-networks', { id, label })
export const removeTrustedNetwork = (id) => request('DELETE', `/api/v1/hive/trusted-networks/${encodeURIComponent(id)}`)
