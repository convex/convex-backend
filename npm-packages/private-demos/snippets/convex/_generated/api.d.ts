/* eslint-disable */
/**
 * Generated `api` utility.
 *
 * THIS CODE IS AUTOMATICALLY GENERATED.
 *
 * To regenerate, run `npx convex dev`.
 * @module
 */

import type {
  ApiFromModules,
  FilterApi,
  FunctionReference,
} from "convex/server";
import type * as actionsArgsWithValidation from "../actionsArgsWithValidation.js";
import type * as actionsCircularError from "../actionsCircularError.js";
import type * as actionsCircularErrorFixedResults from "../actionsCircularErrorFixedResults.js";
import type * as actionsCircularErrorFixedReturn from "../actionsCircularErrorFixedReturn.js";
import type * as actionsConstructor from "../actionsConstructor.js";
import type * as actionsContext from "../actionsContext.js";
import type * as actionsContextRunMutation from "../actionsContextRunMutation.js";
import type * as actionsNPM from "../actionsNPM.js";
import type * as actionsNode from "../actionsNode.js";
import type * as actionsScheduleFromMutation from "../actionsScheduleFromMutation.js";
import type * as applicationErrors from "../applicationErrors.js";
import type * as authFunctions from "../authFunctions.js";
import type * as authFunctionsFields from "../authFunctionsFields.js";
import type * as authFunctionsFieldsJS from "../authFunctionsFieldsJS.js";
import type * as bestPractices_helperFunctions from "../bestPractices/helperFunctions.js";
import type * as bestPractices_index from "../bestPractices/index.js";
import type * as bestPracticesHelpersTeams from "../bestPracticesHelpersTeams.js";
import type * as bestPracticesHelpersTeamsJS from "../bestPracticesHelpersTeamsJS.js";
import type * as clerkMessages from "../clerkMessages.js";
import type * as counter from "../counter.js";
import type * as crons from "../crons.js";
import type * as deletingFiles from "../deletingFiles.js";
import type * as fileMetadata from "../fileMetadata.js";
import type * as filesInMutations from "../filesInMutations.js";
import type * as foods from "../foods.js";
import type * as functions from "../functions.js";
import type * as goClientExample from "../goClientExample.js";
import type * as httpActionConstructor from "../httpActionConstructor.js";
import type * as httpActionExample from "../httpActionExample.js";
import type * as images from "../images.js";
import type * as imagesJS from "../imagesJS.js";
import type * as imagesMetadata from "../imagesMetadata.js";
import type * as internalFunctionsCall from "../internalFunctionsCall.js";
import type * as internalFunctionsDefinitionWithoutValidation from "../internalFunctionsDefinitionWithoutValidation.js";
import type * as internalFunctionsDefinitionWithoutValidationJS from "../internalFunctionsDefinitionWithoutValidationJS.js";
import type * as messages from "../messages.js";
import type * as mutationsArgsWithValidation from "../mutationsArgsWithValidation.js";
import type * as mutationsArgsWithoutValidation from "../mutationsArgsWithoutValidation.js";
import type * as mutationsArgsWithoutValidationJS from "../mutationsArgsWithoutValidationJS.js";
import type * as mutationsConstructor from "../mutationsConstructor.js";
import type * as mutationsContext from "../mutationsContext.js";
import type * as mutationsContextDB from "../mutationsContextDB.js";
import type * as mutationsExample from "../mutationsExample.js";
import type * as mutationsHelper from "../mutationsHelper.js";
import type * as mutationsHelperJS from "../mutationsHelperJS.js";
import type * as mutationsNPM from "../mutationsNPM.js";
import type * as myFunctions from "../myFunctions.js";
import type * as myMutations from "../myMutations.js";
import type * as payments from "../payments.js";
import type * as plans from "../plans.js";
import type * as queriesArgsWithValidation from "../queriesArgsWithValidation.js";
import type * as queriesArgsWithoutValidation from "../queriesArgsWithoutValidation.js";
import type * as queriesArgsWithoutValidationJS from "../queriesArgsWithoutValidationJS.js";
import type * as queriesConstructor from "../queriesConstructor.js";
import type * as queriesContext from "../queriesContext.js";
import type * as queriesContextDB from "../queriesContextDB.js";
import type * as queriesExample from "../queriesExample.js";
import type * as queriesHelper from "../queriesHelper.js";
import type * as queriesHelperJS from "../queriesHelperJS.js";
import type * as queriesNPM from "../queriesNPM.js";
import type * as readingDataAverage from "../readingDataAverage.js";
import type * as readingDataDbGet from "../readingDataDbGet.js";
import type * as readingDataDbQuery from "../readingDataDbQuery.js";
import type * as readingDataGroupByJS from "../readingDataGroupByJS.js";
import type * as readingDataGroupByTS from "../readingDataGroupByTS.js";
import type * as readingDataJoin from "../readingDataJoin.js";
import type * as schemasCircular from "../schemasCircular.js";
import type * as tasks from "../tasks.js";
import type * as tsGeneration from "../tsGeneration.js";
import type * as typescriptContextTypes from "../typescriptContextTypes.js";
import type * as typescriptSystemFieldsTypes from "../typescriptSystemFieldsTypes.js";
import type * as typescriptValidatorTypes from "../typescriptValidatorTypes.js";
import type * as typescriptWithSchema from "../typescriptWithSchema.js";
import type * as typescriptWithValidation from "../typescriptWithValidation.js";
import type * as typescriptWithoutValidation from "../typescriptWithoutValidation.js";
import type * as userHelpers from "../userHelpers.js";
import type * as userHelpersJS from "../userHelpersJS.js";
import type * as vectorSearch from "../vectorSearch.js";
import type * as vectorSearch2 from "../vectorSearch2.js";
import type * as workosFunctions from "../workosFunctions.js";
import type * as writingDataDelete from "../writingDataDelete.js";
import type * as writingDataInsert from "../writingDataInsert.js";
import type * as writingDataPatch from "../writingDataPatch.js";
import type * as writingDataReplace from "../writingDataReplace.js";

/**
 * A utility for referencing Convex functions in your app's API.
 *
 * Usage:
 * ```js
 * const myFunctionReference = api.myModule.myFunction;
 * ```
 */
declare const fullApi: ApiFromModules<{
  actionsArgsWithValidation: typeof actionsArgsWithValidation;
  actionsCircularError: typeof actionsCircularError;
  actionsCircularErrorFixedResults: typeof actionsCircularErrorFixedResults;
  actionsCircularErrorFixedReturn: typeof actionsCircularErrorFixedReturn;
  actionsConstructor: typeof actionsConstructor;
  actionsContext: typeof actionsContext;
  actionsContextRunMutation: typeof actionsContextRunMutation;
  actionsNPM: typeof actionsNPM;
  actionsNode: typeof actionsNode;
  actionsScheduleFromMutation: typeof actionsScheduleFromMutation;
  applicationErrors: typeof applicationErrors;
  authFunctions: typeof authFunctions;
  authFunctionsFields: typeof authFunctionsFields;
  authFunctionsFieldsJS: typeof authFunctionsFieldsJS;
  "bestPractices/helperFunctions": typeof bestPractices_helperFunctions;
  "bestPractices/index": typeof bestPractices_index;
  bestPracticesHelpersTeams: typeof bestPracticesHelpersTeams;
  bestPracticesHelpersTeamsJS: typeof bestPracticesHelpersTeamsJS;
  clerkMessages: typeof clerkMessages;
  counter: typeof counter;
  crons: typeof crons;
  deletingFiles: typeof deletingFiles;
  fileMetadata: typeof fileMetadata;
  filesInMutations: typeof filesInMutations;
  foods: typeof foods;
  functions: typeof functions;
  goClientExample: typeof goClientExample;
  httpActionConstructor: typeof httpActionConstructor;
  httpActionExample: typeof httpActionExample;
  images: typeof images;
  imagesJS: typeof imagesJS;
  imagesMetadata: typeof imagesMetadata;
  internalFunctionsCall: typeof internalFunctionsCall;
  internalFunctionsDefinitionWithoutValidation: typeof internalFunctionsDefinitionWithoutValidation;
  internalFunctionsDefinitionWithoutValidationJS: typeof internalFunctionsDefinitionWithoutValidationJS;
  messages: typeof messages;
  mutationsArgsWithValidation: typeof mutationsArgsWithValidation;
  mutationsArgsWithoutValidation: typeof mutationsArgsWithoutValidation;
  mutationsArgsWithoutValidationJS: typeof mutationsArgsWithoutValidationJS;
  mutationsConstructor: typeof mutationsConstructor;
  mutationsContext: typeof mutationsContext;
  mutationsContextDB: typeof mutationsContextDB;
  mutationsExample: typeof mutationsExample;
  mutationsHelper: typeof mutationsHelper;
  mutationsHelperJS: typeof mutationsHelperJS;
  mutationsNPM: typeof mutationsNPM;
  myFunctions: typeof myFunctions;
  myMutations: typeof myMutations;
  payments: typeof payments;
  plans: typeof plans;
  queriesArgsWithValidation: typeof queriesArgsWithValidation;
  queriesArgsWithoutValidation: typeof queriesArgsWithoutValidation;
  queriesArgsWithoutValidationJS: typeof queriesArgsWithoutValidationJS;
  queriesConstructor: typeof queriesConstructor;
  queriesContext: typeof queriesContext;
  queriesContextDB: typeof queriesContextDB;
  queriesExample: typeof queriesExample;
  queriesHelper: typeof queriesHelper;
  queriesHelperJS: typeof queriesHelperJS;
  queriesNPM: typeof queriesNPM;
  readingDataAverage: typeof readingDataAverage;
  readingDataDbGet: typeof readingDataDbGet;
  readingDataDbQuery: typeof readingDataDbQuery;
  readingDataGroupByJS: typeof readingDataGroupByJS;
  readingDataGroupByTS: typeof readingDataGroupByTS;
  readingDataJoin: typeof readingDataJoin;
  schemasCircular: typeof schemasCircular;
  tasks: typeof tasks;
  tsGeneration: typeof tsGeneration;
  typescriptContextTypes: typeof typescriptContextTypes;
  typescriptSystemFieldsTypes: typeof typescriptSystemFieldsTypes;
  typescriptValidatorTypes: typeof typescriptValidatorTypes;
  typescriptWithSchema: typeof typescriptWithSchema;
  typescriptWithValidation: typeof typescriptWithValidation;
  typescriptWithoutValidation: typeof typescriptWithoutValidation;
  userHelpers: typeof userHelpers;
  userHelpersJS: typeof userHelpersJS;
  vectorSearch: typeof vectorSearch;
  vectorSearch2: typeof vectorSearch2;
  workosFunctions: typeof workosFunctions;
  writingDataDelete: typeof writingDataDelete;
  writingDataInsert: typeof writingDataInsert;
  writingDataPatch: typeof writingDataPatch;
  writingDataReplace: typeof writingDataReplace;
}>;
export declare const api: FilterApi<
  typeof fullApi,
  FunctionReference<any, "public">
>;
export declare const internal: FilterApi<
  typeof fullApi,
  FunctionReference<any, "internal">
>;
