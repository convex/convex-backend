// Be sure to change these settings in dprint.json as well
module.exports = {
  proseWrap: "always",
  trailingComma: "all",
  overrides: [
    {
      files: [".mergify.yml"],
      options: {
        proseWrap: "preserve",
      },
    },
  ],
};
