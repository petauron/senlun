<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'

interface Props {
  visibilityHeight?: number
}

const props = withDefaults(defineProps<Props>(), {
  visibilityHeight: 1,
})

const emit = defineEmits<{
  scrolled: [boolean]
}>()

const show = ref(false)

function handleScroll() {
  const scrolled = window.scrollY > props.visibilityHeight
  show.value = scrolled
  emit('scrolled', scrolled)
}

function scrollToTop() {
  window.scrollTo({ top: 0, behavior: 'smooth' })
}

onMounted(() => {
  window.addEventListener('scroll', handleScroll, { passive: true })
  handleScroll()
})

onUnmounted(() => {
  window.removeEventListener('scroll', handleScroll)
})
</script>

<template>
  <Transition
    enter-active-class="transition-all duration-200"
    enter-from-class="opacity-0 translate-y-2"
    enter-to-class="opacity-100 translate-y-0"
    leave-active-class="transition-all duration-200"
    leave-from-class="opacity-100 translate-y-0"
    leave-to-class="opacity-0 translate-y-2"
  >
    <button
      v-show="show"
      type="button"
      class="fixed bottom-8 right-8 z-40 size-10 cursor-pointer rounded-full bg-background border shadow-sm flex items-center justify-center text-foreground hover:bg-accent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      aria-label="返回顶部"
      @click="scrollToTop"
    >
      <svg
        aria-hidden="true"
        xmlns="http://www.w3.org/2000/svg"
        width="20"
        height="20"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <path d="M12 19V5M5 12l7-7 7 7" />
      </svg>
    </button>
  </Transition>
</template>
