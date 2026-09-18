import { request } from './client.js'

export const getUpdateState = () => request('GET', '/api/v1/update')
// The JSON body is deliberate: it makes this a preflighted request, so a
// foreign page cannot trigger a self-update by form post.
export const applyUpdate = () => request('POST', '/api/v1/update/apply', { confirm: true })
