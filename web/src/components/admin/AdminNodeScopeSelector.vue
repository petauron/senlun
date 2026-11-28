<script setup lang="ts">
import type { ManagedNode } from '@/utils/admin'
import type { AssignableNodeOption, NodeScopeMode } from '@/utils/adminEditing'
import { computed, nextTick, ref, watch } from 'vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  applyFilteredNodeSelection,
  filterAssignableActiveNodes,
  pruneUnavailableNodes,
} from '@/utils/adminEditing'

const props = withDefaults(defineProps<{
  idPrefix: string
  legend: string
  nodes: readonly ManagedNode[]
  options: readonly AssignableNodeOption[]
  scopeMode: NodeScopeMode
  selectedIds: readonly string[]
  disabled?: boolean
  needsGlobalConfirmation: boolean
  confirmGlobalScope: boolean
  globalConfirmWarning: string
  resetNonce: number
  compact?: boolean
}>(), {
  disabled: false,
  compact: false,
})

const emit = defineEmits<{
  'update:scopeMode': [value: NodeScopeMode]
  'update:selectedIds': [value: string[]]
  'update:confirmGlobalScope': [value: boolean]
}>()

const searchQuery = ref('')
const selectedOnly = ref(false)
const searchInputWrapRef = ref<HTMLElement | null>(null)

watch(() => props.resetNonce, () => {
  searchQuery.value = ''
  selectedOnly.value = false
})

watch(searchQuery, () => {
  if (selectedOnly.value)
    selectedOnly.value = false
})

const selectedSet = computed(() => new Set(props.selectedIds))
const activeOptions = computed(() => props.options.filter(option => !option.revoked && !option.missing))
const unavailableOptions = computed(() => props.options.filter(option => option.revoked || option.missing))
const filteredActiveOptions = computed(() => filterAssignableActiveNodes(
  props.options,
  props.nodes,
  searchQuery.value,
  props.selectedIds,
  selectedOnly.value,
))

const activeIdSet = computed(() => new Set(activeOptions.value.map(option => option.id)))
const selectedActiveCount = computed(() => props.selectedIds.filter(id => activeIdSet.value.has(id)).length)
const selectedUnavailableCount = computed(() => props.selectedIds.filter(id => !activeIdSet.value.has(id)).length)
const visibleSelectedActiveCount = computed(() => filteredActiveOptions.value.filter(option => selectedSet.value.has(option.id)).length)
const hiddenSelectedActiveCount = computed(() => Math.max(0, selectedActiveCount.value - visibleSelectedActiveCount.value))
const canSelectAllFiltered = computed(() => filteredActiveOptions.value.some(option => !selectedSet.value.has(option.id)))
const canDeselectFiltered = computed(() => filteredActiveOptions.value.some(option => selectedSet.value.has(option.id)))
const nodeRegionMap = computed(() => new Map(props.nodes.map(node => [node.id, node.region])))
const listGridClass = computed(() => props.compact
  ? 'space-y-1.5'
  : 'grid gap-1.5 sm:grid-cols-2 lg:grid-cols-3')
const scopeBadgeLabel = computed(() => {
  if (props.scopeMode === 'all')
    return '所有活动节点'
  return `指定节点 · 已选 ${selectedActiveCount.value} 台`
})
const hasSearchFilter = computed(() => searchQuery.value.trim().length > 0)
const selectActionLabel = computed(() => selectedOnly.value
  ? '全选当前列表'
  : '全选筛选结果')
const deselectActionLabel = computed(() => selectedOnly.value
  ? '取消勾选全部已选'
  : '取消勾选筛选结果')
const selectedOnlyToggleLabel = computed(() => {
  if (!selectedOnly.value)
    return `仅查看已选（${selectedActiveCount.value}）`
  return hasSearchFilter.value
    ? `返回筛选结果（已选 ${selectedActiveCount.value}）`
    : `显示全部（已选 ${selectedActiveCount.value}）`
})
const selectionSummaryText = computed(() => {
  let summary = `已选 ${selectedActiveCount.value} 台活动节点`
  if (selectedOnly.value && hasSearchFilter.value)
    summary = `已选 ${selectedActiveCount.value} 台活动节点（当前显示全部已选，退出后恢复筛选）`
  else if (hiddenSelectedActiveCount.value > 0)
    summary = `已选 ${selectedActiveCount.value} 台活动节点（另有 ${hiddenSelectedActiveCount.value} 台已选未在当前筛选中）`
  return selectedUnavailableCount.value > 0
    ? `${summary} · 含 ${selectedUnavailableCount.value} 个失效节点`
    : summary
})

function setScopeMode(next: NodeScopeMode): void {
  if (props.disabled || props.scopeMode === next)
    return
  emit('update:scopeMode', next)
}

function toggleNode(id: string, checked: boolean): void {
  if (props.disabled)
    return
  if (checked) {
    if (selectedSet.value.has(id))
      return
    emit('update:selectedIds', [...props.selectedIds, id])
    return
  }
  emit('update:selectedIds', props.selectedIds.filter(item => item !== id))
}

function selectFiltered(): void {
  if (props.disabled || !canSelectAllFiltered.value)
    return
  emit('update:selectedIds', applyFilteredNodeSelection(
    props.selectedIds,
    filteredActiveOptions.value.map(option => option.id),
    'select',
  ))
}

function deselectFiltered(): void {
  if (props.disabled || !canDeselectFiltered.value)
    return
  emit('update:selectedIds', applyFilteredNodeSelection(
    props.selectedIds,
    filteredActiveOptions.value.map(option => option.id),
    'deselect',
  ))
}

function clearUnavailableNodes(): void {
  if (props.disabled || selectedUnavailableCount.value === 0)
    return
  emit('update:selectedIds', pruneUnavailableNodes(props.selectedIds, props.nodes))
}

function clearSearch(): void {
  searchQuery.value = ''
  void nextTick(() => {
    const input = searchInputWrapRef.value?.querySelector('input')
    input?.focus()
  })
}
</script>

<template>
  <fieldset :disabled="disabled" class="space-y-3 rounded-md border p-3">
    <legend class="px-1 text-sm font-medium">
      {{ legend }}
    </legend>
    <div class="flex flex-wrap items-center justify-between gap-2">
      <p class="text-xs text-muted-foreground">
        选择范围模式；模式仅用于表单编辑，不改变服务端配置结构。
      </p>
      <Badge :variant="scopeMode === 'all' ? 'default' : 'secondary'">
        {{ scopeBadgeLabel }}
      </Badge>
    </div>
    <div
      role="radiogroup"
      :aria-label="`${legend}范围模式`"
      class="grid gap-2"
      :class="compact ? 'grid-cols-1' : 'sm:grid-cols-2'"
    >
      <div
        class="rounded-md border p-2.5 text-sm transition-colors"
        :class="scopeMode === 'scoped' ? 'border-primary bg-secondary/70' : 'border-border bg-background'"
      >
        <label class="flex min-h-9 cursor-pointer items-center gap-2 font-medium">
          <input
            type="radio"
            :name="`${idPrefix}-scope-mode`"
            value="scoped"
            :checked="scopeMode === 'scoped'"
            :aria-describedby="`${idPrefix}-scope-scoped-desc`"
            class="size-4 shrink-0 accent-primary"
            @change="setScopeMode('scoped')"
          >
          指定节点
        </label>
        <p :id="`${idPrefix}-scope-scoped-desc`" class="pl-6 text-xs text-muted-foreground">
          仅应用于下方勾选的活动节点；清空筛选或取消选择不会自动变为全局。
        </p>
      </div>
      <div
        class="rounded-md border p-2.5 text-sm transition-colors"
        :class="scopeMode === 'all' ? 'border-primary bg-secondary/70' : 'border-border bg-background'"
      >
        <label class="flex min-h-9 cursor-pointer items-center gap-2 font-medium">
          <input
            type="radio"
            :name="`${idPrefix}-scope-mode`"
            value="all"
            :checked="scopeMode === 'all'"
            :aria-describedby="`${idPrefix}-scope-all-desc`"
            class="size-4 shrink-0 accent-primary"
            @change="setScopeMode('all')"
          >
          所有活动节点（含之后注册）
        </label>
        <p :id="`${idPrefix}-scope-all-desc`" class="pl-6 text-xs text-muted-foreground">
          提交前仍保留刚才勾选的指定节点草稿，切回“指定节点”即可还原。
        </p>
      </div>
    </div>

    <template v-if="scopeMode === 'scoped'">
      <div class="space-y-2">
        <div class="flex flex-wrap items-center gap-2">
          <div ref="searchInputWrapRef" class="relative min-w-0 flex-1 basis-40">
            <Input
              :id="`${idPrefix}-search`"
              v-model="searchQuery"
              type="text"
              class="h-10 pr-9"
              :aria-label="`筛选${legend}`"
              placeholder="按名称、地区或分组筛选节点"
            />
            <button
              v-if="searchQuery"
              type="button"
              aria-label="清空搜索词"
              class="absolute top-1/2 right-1 inline-flex size-8 -translate-y-1/2 items-center justify-center rounded-sm text-sm text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              :disabled="disabled"
              @click="clearSearch"
            >
              ×
            </button>
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <Button type="button" variant="outline" size="sm" class="min-h-10" :disabled="disabled || !canSelectAllFiltered" @click="selectFiltered">
              {{ selectActionLabel }}
            </Button>
            <Button type="button" variant="outline" size="sm" class="min-h-10" :disabled="disabled || !canDeselectFiltered" @click="deselectFiltered">
              {{ deselectActionLabel }}
            </Button>
          </div>
        </div>

        <div class="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
          <p role="status">
            {{ selectionSummaryText }}
          </p>
          <Button
            type="button"
            variant="outline"
            size="xs"
            class="min-h-8"
            :aria-pressed="selectedOnly"
            :disabled="disabled"
            @click="selectedOnly = !selectedOnly"
          >
            {{ selectedOnlyToggleLabel }}
          </Button>
        </div>

        <div class="max-h-56 overflow-y-auto rounded-md border p-2.5">
          <div v-if="filteredActiveOptions.length" :class="listGridClass">
            <div
              v-for="node in filteredActiveOptions"
              :key="node.id"
              class="flex min-h-9 items-center justify-between gap-2"
            >
              <label class="flex min-h-9 min-w-0 flex-1 items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  :value="node.id"
                  :checked="selectedSet.has(node.id)"
                  class="size-4 accent-primary"
                  @change="toggleNode(node.id, ($event.target as HTMLInputElement).checked)"
                >
                <span class="min-w-0 break-words">{{ node.label }}</span>
              </label>
              <span v-if="nodeRegionMap.get(node.id)" class="shrink-0 text-xs text-muted-foreground">{{ nodeRegionMap.get(node.id) }}</span>
            </div>
          </div>
          <p v-else class="py-2 text-center text-xs text-muted-foreground">
            <template v-if="activeOptions.length === 0">
              当前暂无可分配的活动节点。
            </template>
            <template v-else-if="selectedOnly && selectedActiveCount === 0">
              当前尚未勾选任何活动节点。
            </template>
            <template v-else>
              当前筛选无匹配节点。
            </template>
          </p>
        </div>

        <div
          v-if="unavailableOptions.length"
          class="space-y-2 rounded-md border p-2.5 text-xs"
          :class="selectedUnavailableCount > 0 ? 'border-destructive/40' : 'border-border'"
        >
          <div class="flex flex-wrap items-center justify-between gap-2">
            <span :class="selectedUnavailableCount > 0 ? 'text-destructive' : 'text-muted-foreground'">
              {{ selectedUnavailableCount > 0
                ? '当前已选包含已撤销或不存在的节点，请取消勾选或移除后再保存。'
                : '以下历史关联节点已撤销或不存在（已取消勾选，保存后生效）：' }}
            </span>
            <Button
              v-if="selectedUnavailableCount > 0"
              type="button"
              variant="outline"
              size="sm"
              class="min-h-10"
              :disabled="disabled"
              @click="clearUnavailableNodes"
            >
              移除失效节点
            </Button>
          </div>
          <div class="space-y-1.5">
            <label
              v-for="node in unavailableOptions"
              :key="node.id"
              class="flex min-h-9 items-center gap-2 text-sm text-destructive"
            >
              <input
                type="checkbox"
                :value="node.id"
                :checked="selectedSet.has(node.id)"
                class="size-4 accent-primary"
                @change="toggleNode(node.id, ($event.target as HTMLInputElement).checked)"
              >
              <span class="min-w-0 break-words">{{ node.label }}</span>
            </label>
          </div>
        </div>
      </div>
    </template>

    <div v-if="needsGlobalConfirmation" class="space-y-2 rounded-md border border-destructive/40 p-3 text-xs">
      <p class="text-destructive">
        {{ globalConfirmWarning }}
      </p>
      <label class="flex min-h-9 items-center gap-2 text-sm">
        <input
          type="checkbox"
          :checked="confirmGlobalScope"
          class="size-4 accent-primary"
          @change="emit('update:confirmGlobalScope', ($event.target as HTMLInputElement).checked)"
        >
        确认改为应用于所有活动节点
      </label>
    </div>
  </fieldset>
</template>
