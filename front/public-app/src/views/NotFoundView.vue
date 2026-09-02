<script setup lang="ts">
import RouteStateMessage from '@/components/routing/RouteStateMessage.vue'
import { useI18n } from '@/i18n'

const { t } = useI18n()
</script>

<template>
  <RouteStateMessage
    code="404"
    :title="t('notFound.title')"
    :message="t('notFound.message')"
  />
</template>
