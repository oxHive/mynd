import { request } from './client.js'

export const startSession = (project, layer = 'workspace') =>
  request('POST', '/api/v1/audit-sessions', { project, layer })
export const getSession = () => request('GET', '/api/v1/audit-sessions/current')
export const reviseSession = (proposalId, feedback) =>
  request('POST', '/api/v1/audit-sessions/current/revise', { proposal_id: proposalId, feedback })
export const endSession = () => request('DELETE', '/api/v1/audit-sessions/current')
export const listProposals = () => request('GET', '/api/v1/audit-proposals')
export const applyProposal = (id) => request('POST', `/api/v1/audit-proposals/${id}/apply`)
export const rejectProposal = (id) => request('POST', `/api/v1/audit-proposals/${id}/reject`)
