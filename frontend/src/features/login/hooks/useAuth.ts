import { useMutation } from '@tanstack/react-query';
import { loginApi } from '../api/login';
import type { LoginParams } from '../types';

export const useLoginMutation = () => {
   return useMutation({
      mutationFn: (data: LoginParams) => loginApi.login(data),
   });
};