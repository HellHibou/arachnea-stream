<script setup lang="ts">
import { useI18n } from '@/i18n'

const { t } = useI18n()
</script>

<template>
  <v-container class="fill-height" fluid>
    <v-row align="center" justify="center">
      <v-col cols="12" sm="8" md="6" lg="4">
        <v-card class="text-center">
          <v-card-title class="text-h4">404</v-card-title>
          <v-card-text>
            <h1 class="text-h5 mb-3">{{ t('notFound.title') }}</h1>
            <p class="text-body-1">{{ t('notFound.message') }}</p>
          </v-card-text>
        </v-card>
      </v-col>
    </v-row>
  </v-container>
</template>
