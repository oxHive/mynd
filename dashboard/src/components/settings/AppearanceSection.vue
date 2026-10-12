<script setup>
import { computed } from 'vue'
import { useThemeStore } from '../../stores/theme.js'
import { useFontScaleStore, MIN_PERCENT, MAX_PERCENT, DEFAULT_PERCENT } from '../../stores/fontScale.js'
import SegmentedControl from '../shared/SegmentedControl.vue'
import SettingRow from './SettingRow.vue'

const theme = useThemeStore()
const fontScale = useFontScaleStore()

const mode = computed({ get: () => theme.mode, set: v => theme.setMode(v) })
const pct = computed(() => ((fontScale.percent - MIN_PERCENT) / (MAX_PERCENT - MIN_PERCENT)) * 100 + '%')

const THEME_OPTIONS = [
  { value: 'system', label: 'System', icon: 'computer' },
  { value: 'light', label: 'Light', icon: 'light_mode' },
  { value: 'dark', label: 'Dark', icon: 'dark_mode' },
]
</script>

<template>
  <div class="rounded-xl border border-line bg-card px-[18px] py-1">
    <SettingRow label="Theme"><SegmentedControl v-model="mode" :options="THEME_OPTIONS" label="Theme" /></SettingRow>
    <hr class="m-0 h-px border-0 bg-line" />
    <SettingRow label="Text size" hint="Scales text, spacing and layout.">
      <div class="flex min-w-0 items-center gap-2.5">
        <div class="flex min-w-0 max-w-[240px] flex-[1_1_140px] items-center gap-2.5">
          <span class="font-headline text-[11px] font-bold leading-none text-fg-faint">A</span>
          <input
            :value="fontScale.percent"
            type="range"
            class="range min-w-[60px] flex-1"
            :min="MIN_PERCENT"
            :max="MAX_PERCENT"
            step="5"
            aria-label="Text size"
            :style="{ '--pct': pct }"
            @input="fontScale.setPercent(+$event.target.value)"
          />
          <span class="font-headline text-[17px] font-bold leading-none text-fg-faint">A</span>
        </div>
        <span class="w-10 text-right font-headline text-[13px] tabular-nums text-fg-strong">{{ fontScale.percent }}%</span>
        <button type="button" class="hm-btn hm-btn-ghost hm-btn-sm" :disabled="fontScale.percent === DEFAULT_PERCENT" @click="fontScale.reset">Reset</button>
      </div>
    </SettingRow>
  </div>
</template>
