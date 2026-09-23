<script setup>
import { computed, ref } from 'vue'
import { useMemoriesStore } from '../stores/memories.js'
import { useGraphStore } from '../stores/graph.js'
import { useAnalyticsStore } from '../stores/analytics.js'
import { useUiStore } from '../stores/ui.js'
import { buildBuckets, useAnalytics } from '../lib/analytics.js'
import StatCard from '../components/analytics/StatCard.vue'
import AnalyticsPanel from '../components/analytics/AnalyticsPanel.vue'
import SplitBarRow from '../components/analytics/SplitBarRow.vue'
import GrowthChart from '../components/analytics/GrowthChart.vue'
import LayerBadge from '../components/shared/LayerBadge.vue'
import SessionLogRow from '../components/analytics/SessionLogRow.vue'
import EmptyState from '../components/shared/EmptyState.vue'

const memoriesStore = useMemoriesStore()
const graphStore = useGraphStore()
const analyticsStore = useAnalyticsStore()
const ui = useUiStore()

const memories = computed(() => memoriesStore.all)
const edges = computed(() => graphStore.edges)
const now = ref(Math.floor(Date.now() / 1000))

const a = useAnalytics(memories, edges, now)

const RANGES = [{ k: '30d', d: 30 }, { k: '90d', d: 90 }, { k: 'All', d: null }]
const range = ref('90d')
const growth = computed(() => buildBuckets(memories.value, RANGES.find((r) => r.k === range.value).d, now.value))

const pct = (v, of) => (of ? (v / of) * 100 : 0)

function openMemory(id) {
  const mem = memoriesStore.all.find((m) => m.id === id)
  if (!mem) return
  memoriesStore.select(mem)
  ui.requestActiveView('memories')
}
</script>

<template>
  <div class="flex-1 overflow-y-auto px-8 py-8">
    <h2 class="mb-1 font-medium" style="font-size:16px; color:var(--hm-text-primary)">Analytics</h2>
    <p class="mb-6" style="font-size:12px; color:var(--hm-text-tertiary)">How your memory store is growing and holding together</p>

    <div class="analytics-inner">
      <div class="statgrid">
        <StatCard label="Total memories" :value="a.total"
          :sub="[a.personal + ' personal', a.workspace + ' workspace', a.org > 0 ? a.org + ' org' : null].filter(Boolean).join(' · ')" />
        <StatCard label="Added last 90 days" :value="a.in90" :trend="a.in90 - a.prev90" :sub="`vs ${a.prev90} prior 90`" />
        <StatCard label="Connections" :value="a.accepted" :sub="`${a.pending} awaiting review`" />
        <StatCard label="Distinct tags" :value="a.distinctTags" :sub="`${a.tagsPerMemory} tags per memory`" />
      </div>

      <AnalyticsPanel title="Memory growth" hint="Cumulative store size, with new memories per period">
        <template #right>
          <div class="anrange">
            <button v-for="r in RANGES" :key="r.k" class="anrange__btn" :class="{ active: range === r.k }" @click="range = r.k">{{ r.k }}</button>
          </div>
        </template>
        <GrowthChart v-if="growth.buckets.length" :buckets="growth.buckets" :step="growth.step" />
        <EmptyState v-else message="No memories yet" />
        <div v-if="growth.buckets.length" class="anlegend">
          <span class="anlegend__item"><span class="sw" style="background:var(--hm-org)"></span>Total stored</span>
          <span class="anlegend__item"><span class="sw" style="background:var(--hm-accent)"></span>Added in period</span>
        </div>
      </AnalyticsPanel>

      <AnalyticsPanel title="Top tags" :hint="`${a.distinctTags} distinct tags across the store`">
        <template #right>
          <div class="anlegend">
            <span class="anlegend__item"><span class="sw" style="background:var(--hm-personal)"></span>Personal</span>
            <span class="anlegend__item"><span class="sw" style="background:var(--hm-workspace)"></span>Workspace</span>
            <span v-if="a.org > 0" class="anlegend__item"><span class="sw" style="background:var(--hm-org)"></span>Org</span>
          </div>
        </template>
        <div v-if="a.topTags.length" class="sbrlist">
          <SplitBarRow v-for="t in a.topTags" :key="t.tag" :label="t.tag" :personal="t.personal" :workspace="t.workspace" :org="t.org" :max="a.maxTag" />
        </div>
        <EmptyState v-else message="No tags yet" />
      </AnalyticsPanel>

      <div class="ancol">
        <AnalyticsPanel title="Composition" hint="Memory type and layer mix">
          <template v-if="a.total">
            <div class="compbar">
              <div v-for="t in a.types" :key="t.type" class="compbar__seg" :class="'compbar__seg--' + t.type"
                :style="{ width: pct(t.total, a.total) + '%' }" :title="`${t.type} · ${t.total}`"></div>
            </div>
            <div class="complist">
              <div v-for="t in a.types" :key="t.type" class="comprow">
                <span class="sw" :class="'sw--' + t.type"></span>
                <span class="comprow__k font-mono">{{ t.type }}</span>
                <span class="comprow__pct">{{ Math.round(pct(t.total, a.total)) }}%</span>
                <span class="comprow__n font-mono">{{ t.total }}</span>
              </div>
            </div>
            <hr class="divider" />
            <div class="sbrlist">
              <SplitBarRow v-for="t in a.types" :key="t.type" :label="t.type" :personal="t.personal" :workspace="t.workspace" :org="t.org" :max="a.maxType" />
            </div>
          </template>
          <EmptyState v-else message="No memories yet" />
        </AnalyticsPanel>

        <AnalyticsPanel title="Connection health" :hint="`${a.avgDegree} links per memory on average`">
          <div class="compbar">
            <div class="compbar__seg" style="background:var(--hm-workspace)" :style="{ width: pct(a.accepted, a.edgeTotal) + '%' }"></div>
            <div class="compbar__seg" style="background:var(--hm-accent)" :style="{ width: pct(a.pending, a.edgeTotal) + '%' }"></div>
            <div class="compbar__seg" style="background:var(--hm-border-strong)" :style="{ width: pct(a.rejected, a.edgeTotal) + '%' }"></div>
          </div>
          <div class="complist">
            <div v-for="row in [
              { k: 'accepted', color: 'var(--hm-workspace)', n: a.accepted },
              { k: 'pending', color: 'var(--hm-accent)', n: a.pending },
              { k: 'rejected', color: 'var(--hm-border-strong)', n: a.rejected },
            ]" :key="row.k" class="comprow">
              <span class="sw" :style="{ background: row.color }"></span>
              <span class="comprow__k font-mono">{{ row.k }}</span>
              <span class="comprow__pct">{{ Math.round(pct(row.n, a.edgeTotal)) }}%</span>
              <span class="comprow__n font-mono">{{ row.n }}</span>
            </div>
          </div>
          <hr class="divider" />
          <div class="minigrid">
            <div v-for="o in a.byOrigin" :key="o.t" class="mini">
              <div class="mini__n">{{ o.n }}</div>
              <div class="mini__k font-mono">{{ o.t }}</div>
            </div>
            <div class="mini">
              <div class="mini__n" :class="{ 'is-warn': a.orphans > 0 }">{{ a.orphans }}</div>
              <div class="mini__k font-mono">unlinked</div>
            </div>
          </div>
        </AnalyticsPanel>
      </div>

      <AnalyticsPanel title="Most connected memories" hint="Hubs carry the most context into a session">
        <div v-if="a.hubs.some(h => h.deg > 0)" class="sbrlist">
          <div v-for="{ m, deg } in a.hubs" :key="m.id" class="hubrow" @click="openMemory(m.id)">
            <LayerBadge :layer="m.layer" />
            <span class="hubrow__title">{{ m.title }}</span>
            <div class="hubrow__track"><div class="hubrow__fill" :style="{ width: pct(deg, a.maxDeg) + '%', background: `var(--hm-${m.layer})` }"></div></div>
            <span class="hubrow__n font-mono">{{ deg }}</span>
            <svg class="hubrow__go" width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
              <path d="M6 3.5L10.5 8L6 12.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
            </svg>
          </div>
        </div>
        <EmptyState v-else message="No connections yet" hint="Accept a suggested link, or add a [phrase](mem_id) reference in a memory's content." />
        <div v-if="a.total" class="anfoot">
          {{ a.touched7 }} memories updated in the last 7 days · {{ a.stale }} untouched for over {{ a.staleDays }} days
        </div>
      </AnalyticsPanel>

      <AnalyticsPanel title="Recall sessions" hint="Session-start runs logged by Claude Code with Mynd configured">
        <div v-if="analyticsStore.sessionLogs.length" class="sbrlist">
          <SessionLogRow v-for="log in analyticsStore.sessionLogs" :key="log.id" :log="log" />
        </div>
        <EmptyState v-else message="No session-start runs logged yet"
          hint="This fills in once a Claude Code session with Mynd configured runs its session-start hook." />
      </AnalyticsPanel>
    </div>
  </div>
</template>

<style scoped>
.analytics-inner { max-width: 1120px; min-width: 0; display: grid; grid-template-columns: minmax(0, 1fr); gap: 18px; }

.statgrid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 14px; }

.anrange { display: flex; gap: 2px; background: var(--hm-bg-overlay); border-radius: 4px; padding: 2px; flex-shrink: 0; }
.anrange__btn { font-family: var(--hm-font-mono); font-size: 11px; padding: 4px 10px; border: none; background: none; color: var(--hm-text-secondary); border-radius: 3px; cursor: pointer; transition: background .15s, color .15s; }
.anrange__btn:hover { color: var(--hm-text-primary); }
.anrange__btn.active { background: var(--hm-bg-elevated); color: var(--hm-text-primary); box-shadow: 0 1px 2px rgba(0,0,0,.2); }

.anlegend { display: flex; gap: 14px; flex-shrink: 0; flex-wrap: wrap; }
.anlegend__item { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--hm-text-secondary); white-space: nowrap; margin-top: 10px; }
.sw { width: 9px; height: 9px; border-radius: 2px; flex-shrink: 0; display: inline-block; }
.sw--preference { background: var(--hm-personal); }
.sw--project { background: var(--hm-workspace); }
.sw--history { background: var(--hm-accent); }

.sbrlist { display: flex; flex-direction: column; gap: 7px; }

.ancol { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 18px; align-items: start; }

.compbar { display: flex; height: 10px; border-radius: 2px; overflow: hidden; background: var(--hm-bg-overlay); }
.compbar__seg--preference { background: var(--hm-personal); }
.compbar__seg--project { background: var(--hm-workspace); }
.compbar__seg--history { background: var(--hm-accent); }
.complist { display: flex; flex-direction: column; gap: 6px; margin: 13px 0 16px; }
.comprow { display: grid; grid-template-columns: 9px 1fr auto 30px; align-items: center; gap: 9px; font-size: 12px; }
.comprow__k { color: var(--hm-text-secondary); }
.comprow__pct { color: var(--hm-text-tertiary); font-size: 11.5px; }
.comprow__n { color: var(--hm-text-primary); text-align: right; font-variant-numeric: tabular-nums; }
.divider { border: none; border-top: 0.5px solid var(--hm-border-default); margin: 0 0 16px; }

.minigrid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 10px; }
.mini { background: var(--hm-bg-surface); border: 0.5px solid var(--hm-border-default); border-radius: 4px; padding: 9px 10px; }
.mini__n { font-weight: 700; font-size: 19px; color: var(--hm-text-primary); line-height: 1.1; letter-spacing: -0.03em; }
.mini__n.is-warn { color: var(--hm-accent); }
.mini__k { font-size: 10px; color: var(--hm-text-tertiary); margin-top: 2px; }

.hubrow { display: grid; grid-template-columns: auto minmax(0, 1fr) 180px 26px 18px; align-items: center; gap: 12px; padding: 6px 8px; margin: 0 -8px; border-radius: 4px; cursor: pointer; transition: background .14s; }
.hubrow:hover { background: var(--hm-bg-overlay); }
.hubrow__title { font-size: 13px; color: var(--hm-text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.hubrow__track { height: 8px; border-radius: 2px; background: color-mix(in srgb, var(--hm-border-default) 45%, transparent); overflow: hidden; }
.hubrow__fill { height: 100%; border-radius: 2px; }
.hubrow__n { font-size: 11.5px; color: var(--hm-text-primary); text-align: right; font-variant-numeric: tabular-nums; }
.hubrow__go { color: var(--hm-text-tertiary); flex-shrink: 0; }
.anfoot { margin-top: 16px; padding-top: 13px; border-top: 0.5px solid var(--hm-border-default); font-size: 11.5px; color: var(--hm-text-tertiary); }

@media (max-width: 1080px) {
  .statgrid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .ancol { grid-template-columns: 1fr; }
  .hubrow { grid-template-columns: auto minmax(0, 1fr) 90px 26px 18px; }
}
</style>
