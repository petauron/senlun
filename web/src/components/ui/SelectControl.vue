<script setup lang="ts" generic="T extends string">
import { Icon } from '@iconify/vue'
import { SelectContent, SelectItem, SelectItemIndicator, SelectItemText, SelectPortal, SelectRoot, SelectTrigger, SelectValue, SelectViewport } from 'reka-ui'

defineProps<{ modelValue: T, options: { value: T, label: string }[], disabled?: boolean, ariaLabel?: string, id?: string }>()
const emit = defineEmits<{ 'update:modelValue': [value: T], 'update:open': [value: boolean] }>()
</script>

<template>
  <SelectRoot :model-value="modelValue" :disabled="disabled" @update:open="value => emit('update:open', value)" @update:model-value="value => { if (value !== undefined) emit('update:modelValue', value as T) }">
    <SelectTrigger :id="id" :aria-label="ariaLabel" class="flex min-h-11 min-w-0 items-center justify-between gap-2 rounded-md border border-input bg-background px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50">
      <SelectValue />
      <Icon icon="lucide:chevron-down" class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
    </SelectTrigger>
    <SelectPortal>
      <SelectContent position="popper" :side-offset="4" class="z-50 max-h-64 min-w-[var(--reka-select-trigger-width)] overflow-auto rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-lg">
        <SelectViewport>
          <SelectItem v-for="option in options" :key="option.value" :value="option.value" class="relative flex min-h-9 cursor-default select-none items-center rounded-sm px-3 pr-8 text-sm outline-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground">
            <SelectItemText>{{ option.label }}</SelectItemText>
            <SelectItemIndicator class="absolute right-2"><Icon icon="lucide:check" class="size-4" aria-hidden="true" /></SelectItemIndicator>
          </SelectItem>
        </SelectViewport>
      </SelectContent>
    </SelectPortal>
  </SelectRoot>
</template>
