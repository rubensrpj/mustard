import { User } from "./user";

export class UserService {
  load(): User {
    return new User();
  }
}
