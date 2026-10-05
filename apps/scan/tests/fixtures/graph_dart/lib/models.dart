// Fixture for the kinds-manifest parity test (tests/kinds_parity.rs).
// Exercises every @definition.<kind> the Dart tags.scm emits:
//   class, mixin, enum, extension, method (function_signature), const,
//   field, property (the getter), enum_member.
// It must NOT produce any undeclared kind.
import 'dart:async';

enum Role { admin, member }

const defaultRole = Role.member;

class Account {
  final String id;
  int visits = 0;
  Account(this.id);

  String get label => 'conta $id';

  String describe() => 'account $id';
}

mixin Auditable {
  Future<void> touch();
}

extension AccountFormatting on Account {
  String shout() => describe().toUpperCase();
}

String summarize(Account account) => account.describe();
