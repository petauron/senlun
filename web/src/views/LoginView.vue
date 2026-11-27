<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { Input } from '@/components/ui/input'
import { useAuthStore } from '@/stores/auth'
import { ApiError, getSharedApi } from '@/utils/api'
import { resolveOAuthTotpViewState } from '@/utils/authFlow'
import { loginDestination } from '@/utils/session'

const auth = useAuthStore()
const route = useRoute()
const router = useRouter()
const username = ref('')
const password = ref('')
const code = ref('')
const busy = ref(false)
const error = ref('')
const dismissedOAuthTotp = ref(false)
const oauthTotpExpiredNotice = ref(false)
const setup = computed(() => auth.status?.initialized === false)
const oauthTotpState = computed(() => resolveOAuthTotpViewState(
  route.query.oauth_totp,
  auth.status,
  dismissedOAuthTotp.value,
))
const oauthTotp = computed(() => oauthTotpState.value.active)
const heading = computed(() => setup.value
  ? '初始化 Senlun'
  : oauthTotp.value ? '完成两步验证' : '登录 Senlun')

async function clearOAuthTotpQuery(): Promise<void> {
  if (route.query.oauth_totp === undefined)
    return
  const query = { ...route.query }
  delete query.oauth_totp
  await router.replace({ name: 'login', query })
}

watch(oauthTotpState, (state) => {
  if (state.expired) {
    oauthTotpExpiredNotice.value = true
    void clearOAuthTotpQuery()
  }
}, { immediate: true })

async function returnToStandardLogin(): Promise<void> {
  busy.value = true
  error.value = ''
  try {
    await auth.refresh(true)
    dismissedOAuthTotp.value = true
    oauthTotpExpiredNotice.value = false
    code.value = ''
    await clearOAuthTotpQuery()
  }
  catch (cause) {
    error.value = cause instanceof Error
      ? cause.message
      : String(cause)
  }
  finally {
    busy.value = false
  }
}

async function refresh(): Promise<void> {
  busy.value = true
  error.value = ''
  try {
    await auth.refresh()
  }
  catch (cause) {
    error.value = cause instanceof Error
      ? cause.message
      : String(cause)
  }
  finally { busy.value = false }
}

onMounted(refresh)

async function submit(): Promise<void> {
  busy.value = true
  error.value = ''
  const submittingOAuthTotp = oauthTotp.value
  try {
    const path = setup.value
      ? 'setup'
      : submittingOAuthTotp ? 'oauth/complete' : 'login'
    const body = setup.value
      ? { username: username.value, password: password.value }
      : submittingOAuthTotp
        ? { code: code.value }
        : { username: username.value, password: password.value, code: code.value || undefined }
    await auth.authenticate(path, body)
    password.value = ''
    code.value = ''
    await router.replace(loginDestination(route.query.redirect))
  }
  catch (cause) {
    if (submittingOAuthTotp && cause instanceof ApiError && (cause.statusCode === 401 || cause.statusCode === 403)) {
      try {
        const latest = await auth.refresh(true)
        if (!latest.oauth_totp_required) {
          oauthTotpExpiredNotice.value = true
          code.value = ''
          await clearOAuthTotpQuery()
          return
        }
      }
      catch {
        // Keep the original submission failure visible when status verification fails.
      }
    }
    error.value = cause instanceof Error
      ? cause.message
      : String(cause)
  }
  finally { busy.value = false }
}

async function startOAuth(): Promise<void> {
  busy.value = true
  error.value = ''
  try {
    const { authorization_url } = await getSharedApi().post<{ authorization_url: string }>('auth/oauth/start')
    const url = new URL(authorization_url)
    if (url.protocol !== 'https:' && !(url.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname)))
      throw new Error('OAuth 服务返回了无效的登录地址')
    window.location.assign(url.href)
  }
  catch (cause) {
    error.value = cause instanceof Error
      ? cause.message
      : String(cause)
    busy.value = false
  }
}
</script>

<template>
  <div class="mx-auto max-w-md px-4 py-8">
    <CardX :title="heading" content-class="space-y-4">
      <p v-if="setup" class="text-sm text-muted-foreground">
        创建首个管理员后，初始化入口将关闭。
      </p>
      <p v-else-if="oauthTotp" class="text-sm text-muted-foreground">
        输入身份验证器中的六位验证码以完成 OAuth 登录。
      </p>
      <p v-else-if="oauthTotpExpiredNotice" role="status" class="rounded-md border border-border p-3 text-sm text-muted-foreground">
        OAuth 两步验证会话已过期，请重新使用密码或 OAuth 登录。
      </p>
      <p v-if="error" id="login-error" role="alert" class="rounded-md border border-destructive/40 p-3 text-sm text-destructive">
        {{ error }}
      </p>
      <form v-if="auth.status" class="space-y-4" :aria-busy="busy" :aria-describedby="error ? 'login-error' : undefined" @submit.prevent="submit">
        <template v-if="!oauthTotp">
          <div class="space-y-2">
            <label for="username" class="text-sm font-medium">用户名</label>
            <Input id="username" v-model="username" name="username" autocomplete="username" required maxlength="64" pattern="[A-Za-z0-9_.\-]+" :disabled="busy" />
            <p v-if="setup" class="text-xs text-muted-foreground">
              使用英文字母、数字、下划线、连字符或句点。
            </p>
          </div>
          <div class="space-y-2">
            <label for="password" class="text-sm font-medium">密码</label>
            <Input id="password" v-model="password" name="password" type="password" :autocomplete="setup ? 'new-password' : 'current-password'" required :disabled="busy" />
          </div>
        </template>
        <div v-if="!setup" class="space-y-2">
          <label for="login-code" class="text-sm font-medium">两步验证码{{ oauthTotp ? '' : '（已开启时填写）' }}</label>
          <Input id="login-code" v-model="code" name="code" autocomplete="one-time-code" inputmode="numeric" pattern="[0-9]{6}" maxlength="6" :required="oauthTotp" :disabled="busy" />
        </div>
        <Button class="min-h-11 w-full" type="submit" :disabled="busy">
          {{ busy ? '正在处理…' : setup ? '创建管理员' : '登录' }}
        </Button>
      </form>
      <Button v-else class="min-h-11 w-full" :disabled="busy" @click="refresh">
        {{ busy ? '正在连接…' : '重新连接' }}
      </Button>
      <Button v-if="oauthTotp" variant="outline" class="min-h-11 w-full" :disabled="busy" @click="returnToStandardLogin">
        返回常规登录
      </Button>
      <Button v-if="auth.status?.oauth_enabled && !oauthTotp" variant="outline" class="min-h-11 w-full" :disabled="busy" @click="startOAuth">
        使用 OAuth 登录
      </Button>
      <RouterLink v-if="auth.status?.initialized && !oauthTotp" to="/" class="block text-center text-sm underline underline-offset-4">
        返回监控首页
      </RouterLink>
    </CardX>
  </div>
</template>
