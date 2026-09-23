// Fixture for the kinds-manifest parity test (tests/kinds_parity.rs).
// Exercises every @definition.<kind> the Dart tags.scm emits:
//   class, mixin, enum, extension, method (function_signature), const.
// It must NOT produce any undeclared kind, so it deliberately avoids
// constructs the tags.scm does not capture (fields, enum members, getters).
import 'dart:async';

enum Role { admin, member }

const defaultRole = Role.member;

class Account {
  final String id;
  Account(this.id);

  String describe() => 'account $id';
}

mixin Auditable {
  Future<void> touch();
}

extension AccountFormatting on Account {
  String shout() => describe().toUpperCase();
}

String summarize(Account account) => account.describe();
