# Task: Afficher le score dans EntryDetailsMetadata.vue

## Étapes à réaliser:
1. [ ] Ajouter le champ `score` dans le type `EntryDetails` (front/src/types/entry.ts)
2. [ ] Extraire le `score` dans `normalizeEntryDetails` (front/src/services/rustify.ts)
3. [ ] Ajouter le `score` dans `entryDetailsMetadataPresentation` (front/src/composables/entry-details/entryDetailsMetadataPresentation.ts)
4. [ ] Afficher le `score` dans le template `EntryDetailsMetadata.vue`
5. [ ] Passer le prop `score` depuis ProgramEntryDetails.vue via EntryDetails.vue